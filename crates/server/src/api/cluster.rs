//! The Cluster page: where sandboxes run, whether the images they need are
//! there, and what is going wrong.

use crate::app::App;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::sandbox::{NodeInfo, SandboxInfo, normalize_image};
use axum::Json;
use axum::extract::State;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use uuid::Uuid;

/// An image sandboxes are started from, and what needs it.
struct Wanted {
    /// As an admin wrote it.
    image: String,
    /// In full, for comparing with what nodes report.
    full: String,
    used_by: Vec<Value>,
}

fn want(wanted: &mut Vec<Wanted>, image: &str, kind: &str, name: &str) {
    let full = normalize_image(image);
    let user = json!({ "kind": kind, "name": name });
    match wanted.iter_mut().find(|w| w.full == full) {
        Some(existing) => existing.used_by.push(user),
        None => wanted.push(Wanted { image: image.trim().to_string(), full, used_by: vec![user] }),
    }
}

/// Whether a node holds an image: `present`, `missing`, or `unknown` when the
/// node does not list it but may not be listing everything it has.
fn on_node(node: &NodeInfo, full: &str) -> Value {
    let held = node.images.iter().find(|image| image.names.iter().any(|name| normalize_image(name) == full));
    let state = match held {
        Some(_) => "present",
        None if node.images_complete => "missing",
        None => "unknown",
    };
    json!({ "node": node.name, "state": state, "size_bytes": held.and_then(|image| image.size_bytes) })
}

/// Why an image is failing to pull right now, if a sandbox is stuck on it.
fn pull_error(sandboxes: &[SandboxInfo], full: &str) -> Option<String> {
    sandboxes
        .iter()
        .flat_map(|sandbox| &sandbox.containers)
        .find(|container| container.pull_failed() && normalize_image(&container.image) == full)
        .map(|container| if container.message.is_empty() { container.reason.clone() } else { container.message.clone() })
}

/// Every image sandboxes start from. Images only mean something where
/// sandboxes are containers.
async fn wanted_images(app: &App) -> ApiResult<Vec<Wanted>> {
    let mut wanted = Vec::new();
    if app.driver.name() != "kubernetes" {
        return Ok(wanted);
    }
    want(&mut wanted, &app.cfg.runner_image, "runner", "Runner");
    let harnesses: Vec<(String, String)> = sqlx::query_as("SELECT title, image FROM harnesses ORDER BY title").fetch_all(&app.db).await?;
    for (title, image) in &harnesses {
        want(&mut wanted, image, "harness", title);
    }
    let environments: Vec<(String, String)> = sqlx::query_as("SELECT name, image FROM environments ORDER BY name").fetch_all(&app.db).await?;
    for (name, image) in &environments {
        want(&mut wanted, image, "environment", name);
    }
    Ok(wanted)
}

pub async fn get(State(app): State<App>, actor: AuthUser) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let inspection = app.driver.inspect().await;
    let kubernetes = app.driver.name() == "kubernetes";

    let wanted = wanted_images(&app).await?;
    let images: Vec<Value> = wanted
        .iter()
        .map(|w| {
            let pulls: Vec<&_> = inspection.pulls.iter().filter(|p| normalize_image(&p.image) == w.full).collect();
            json!({
                "image": w.image,
                "used_by": w.used_by,
                "nodes": inspection.nodes.iter().map(|node| on_node(node, &w.full)).collect::<Vec<_>>(),
                "pull_error": pull_error(&inspection.sandboxes, &w.full),
                "pulls": pulls,
            })
        })
        .collect();

    // Say whose each sandbox is: a session's, a model discovery probe's, or nobody's.
    let ids: Vec<Uuid> = inspection.sandboxes.iter().filter_map(|s| s.id).collect();
    let sessions: Vec<(Uuid, String, String)> =
        sqlx::query_as("SELECT id, title, status FROM sessions WHERE id = ANY($1)").bind(&ids).fetch_all(&app.db).await?;
    let sessions: HashMap<Uuid, (String, String)> = sessions.into_iter().map(|(id, title, status)| (id, (title, status))).collect();
    let mut sandboxes = Vec::new();
    for sandbox in &inspection.sandboxes {
        let owner = match sandbox.id {
            Some(id) if sessions.contains_key(&id) => {
                let (title, status) = &sessions[&id];
                json!({ "kind": "session", "session_id": id, "title": title, "status": status })
            }
            Some(id) => match app.probes.harness_of(id) {
                Some(harness_id) => {
                    let title = crate::store::harness_by_id(&app.db, harness_id).await?.map(|h| h.title).unwrap_or_default();
                    json!({ "kind": "probe", "title": title })
                }
                None => json!({ "kind": "orphan" }),
            },
            None => json!({ "kind": "orphan" }),
        };
        let mut value = serde_json::to_value(sandbox).unwrap_or_default();
        value["owner"] = owner;
        sandboxes.push(value);
    }

    Ok(Json(json!({
        "driver": app.driver.name(),
        "checked_at": Utc::now(),
        "namespace": inspection.namespace,
        "live_sessions": app.live.count(),
        "max_sessions": app.cfg.max_sessions,
        "sandbox": kubernetes.then(|| json!({
            "pull_policy": app.cfg.k8s_image_pull_policy,
            "cpu_limit": app.cfg.k8s_cpu_limit,
            "memory_limit": app.cfg.k8s_memory_limit,
            "workspace_size": app.cfg.k8s_workspace_size,
        })),
        "nodes": inspection.nodes,
        "nodes_error": inspection.nodes_error,
        "images": images,
        "sandboxes": sandboxes,
        "sandboxes_error": inspection.sandboxes_error,
        "events": inspection.events,
        "events_error": inspection.events_error,
    })))
}

#[derive(Deserialize)]
pub struct Pull {
    image: String,
}

/// Fetch an image afresh onto every node, so a tag such as `latest` that has
/// moved on in the registry is what the next sandbox starts from.
pub async fn pull(State(app): State<App>, actor: AuthUser, Json(body): Json<Pull>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let full = normalize_image(&body.image);
    let wanted = wanted_images(&app).await?;
    let Some(image) = wanted.iter().find(|w| w.full == full) else {
        return Err(ApiError::bad("only an image a sandbox starts from can be pulled"));
    };
    let nodes = app.driver.pull(&image.image).await.map_err(|err| ApiError::bad(format!("{err:#}")))?;
    tracing::info!(user = %actor.0.id, image = %image.image, nodes, "image pull started");
    Ok(Json(json!({ "nodes": nodes })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::NodeImage;

    fn node(images: &[&str], complete: bool) -> NodeInfo {
        NodeInfo {
            name: "pi".into(),
            images: images.iter().map(|name| NodeImage { names: vec![name.to_string()], size_bytes: Some(7) }).collect(),
            images_complete: complete,
            ..Default::default()
        }
    }

    #[test]
    fn an_image_is_found_on_a_node_however_it_is_spelled() {
        let pi = node(&["docker.io/library/rust:1.90"], true);
        let found = on_node(&pi, &normalize_image("rust:1.90"));
        assert_eq!((found["state"].as_str(), found["size_bytes"].as_u64()), (Some("present"), Some(7)));
        assert_eq!(on_node(&pi, &normalize_image("rust:1.91"))["state"], "missing");
        // A node that may be holding back part of its list cannot rule an image out.
        assert_eq!(on_node(&node(&[], false), &normalize_image("rust:1.90"))["state"], "unknown");
    }

    #[test]
    fn images_named_twice_are_listed_once() {
        let mut wanted = Vec::new();
        want(&mut wanted, "ghcr.io/acme/dev", "harness", "Claude Code");
        want(&mut wanted, "ghcr.io/acme/dev:latest", "environment", "dev");
        assert_eq!(wanted.len(), 1);
        assert_eq!(wanted[0].used_by.len(), 2);
    }
}
