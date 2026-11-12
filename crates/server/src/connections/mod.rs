//! MCP connections: external MCP servers the tool gateway proxies to.
//!
//! Agents never talk to these servers themselves. The gateway lists the
//! tools a connection offers (as `<connection>_<tool>`), checks each call
//! against the session's grants, adds the credential, and forwards it.
//!
//! Every tool is either a read tool (needs `<connection>:read`), a write tool
//! (`<connection>:write`), or hidden. First-class connections ([`presets`])
//! sort their tools automatically; for custom connections an admin decides,
//! and a tool nobody has decided on, or one whose definition has changed
//! since, stays hidden.

pub mod client;
pub mod oauth;
pub mod presets;

use crate::app::App;
use crate::store;
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use client::Endpoint;
use hyperbeam_core::{Grant, Level};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::types::Json;
use std::collections::BTreeMap;

/// Names custom connections may not take: built-in systems and presets.
pub const RESERVED: &[&str] = &["github", "notion", "hyperbeam", "platform", "mcp", "web"];

/// Tools are re-listed in the background when this old.
const STALE_AFTER_SECS: i64 = 6 * 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolLevel {
    Read,
    Write,
    Hidden,
}

impl ToolLevel {
    pub fn grant_level(self) -> Option<Level> {
        match self {
            ToolLevel::Read => Some(Level::Read),
            ToolLevel::Write => Some(Level::Write),
            ToolLevel::Hidden => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Auth {
    /// Header that carries the credential; `None` sends no credential.
    #[serde(default)]
    pub header: Option<String>,
    /// Written before the credential, e.g. `Bearer `.
    #[serde(default)]
    pub prefix: String,
    /// Set when the credential is a token from an OAuth sign-in, which
    /// Hyperbeam keeps fresh, rather than a key an admin pasted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<oauth::OAuth>,
}

/// A tool as the upstream server last described it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
    #[serde(default)]
    pub annotations: Value,
    /// Changes whenever the description, schema or annotations do.
    pub fingerprint: String,
    /// What the preset sorted it into, if anything.
    #[serde(default)]
    pub suggested: Option<ToolLevel>,
    /// The preset's read-only variant also lists it.
    #[serde(default)]
    pub read_only_variant: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    pub level: ToolLevel,
    /// The tool's fingerprint when the admin decided.
    pub fingerprint: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Connection {
    pub name: String,
    pub preset: Option<String>,
    pub title: String,
    pub url: String,
    pub auth: Json<Auth>,
    pub headers: Json<BTreeMap<String, String>>,
    pub options: Json<Value>,
    pub tools: Json<Vec<Tool>>,
    pub approvals: Json<BTreeMap<String, Approval>>,
    pub tools_error: Option<String>,
    pub tools_refreshed_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

/// Where a tool's level came from, for the admin screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolStatus {
    /// An admin decided, and the tool has not changed since.
    Approved,
    /// The preset sorted it.
    Automatic,
    /// Nobody has decided; hidden.
    Pending,
    /// An admin decided, but the tool has changed since; hidden.
    Changed,
}

pub fn secret_key(name: &str) -> String {
    format!("connection/{name}")
}

impl Connection {
    pub fn preset(&self) -> Option<&'static presets::Preset> {
        self.preset.as_deref().and_then(presets::find)
    }

    /// The level a tool is used at, and why. `Hidden` for anything an admin
    /// has not cleared.
    pub fn level_of(&self, tool: &Tool) -> (ToolLevel, ToolStatus) {
        match self.approvals.get(&tool.name) {
            // A vendor's own server may reword a tool; an admin's decision
            // about it stands. A custom server's changed tool is looked at again.
            Some(a) if a.fingerprint == tool.fingerprint || self.preset.is_some() => (a.level, ToolStatus::Approved),
            Some(_) => (ToolLevel::Hidden, ToolStatus::Changed),
            None => match tool.suggested {
                Some(level) => (level, ToolStatus::Automatic),
                None => (ToolLevel::Hidden, ToolStatus::Pending),
            },
        }
    }

    pub fn read_label(&self) -> String {
        self.preset().map(|p| p.read_label.to_string()).unwrap_or_else(|| "Use its read tools".into())
    }

    pub fn write_label(&self) -> String {
        self.preset().map(|p| p.write_label.to_string()).unwrap_or_else(|| "Use its write tools".into())
    }

    /// The name agents see a tool under, after `<connection>_`. A vendor
    /// that prefixes every tool with its own name loses the prefix:
    /// Notion's `notion-search` is `notion_search`, not `notion_notion-search`.
    pub fn short_name<'a>(&self, tool: &'a str) -> &'a str {
        self.preset().map_or(tool, |p| p.short_name(tool))
    }

    /// The full endpoint, credential included.
    pub async fn endpoint(&self, app: &App) -> Result<Endpoint> {
        if self.token_expiring() {
            app.refresh_oauth(&self.name, None).await?;
        }
        let mut headers: Vec<(String, String)> = self.headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if let Some(preset) = self.preset() {
            headers.extend(preset.headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        }
        if let Some(header) = &self.auth.header {
            let secret = store::get_secret(&app.db, &app.secrets, &secret_key(&self.name))
                .await?
                .ok_or_else(|| anyhow!("{} has no credential. An admin can add one under Settings → Integrations.", self.title))?;
            headers.push((header.clone(), format!("{}{secret}", self.auth.prefix)));
        }
        Ok(Endpoint { label: self.title.clone(), url: self.url.clone(), headers })
    }

    /// What an admin sees: everything but the credential.
    pub fn view(&self) -> Value {
        let tools: Vec<Value> = self
            .tools
            .iter()
            .map(|tool| {
                let (level, status) = self.level_of(tool);
                json!({
                    "name": tool.name,
                    "title": tool.title,
                    "description": tool.description,
                    "annotations": tool.annotations,
                    "level": level,
                    "status": status,
                    "suggested": tool.suggested,
                })
            })
            .collect();
        let waiting = self.tools.iter().filter(|t| matches!(self.level_of(t).1, ToolStatus::Pending | ToolStatus::Changed)).count();
        json!({
            "name": self.name,
            "preset": self.preset,
            "title": self.title,
            "url": self.url,
            "auth": {
                "header": self.auth.header,
                "prefix": self.auth.prefix,
                "oauth": self.auth.oauth.as_ref().map(|o| json!({
                    // A client the server registered is not the admin's to edit.
                    "client_id": (!o.registered).then_some(&o.client_id),
                    "scope": o.scope,
                })),
            },
            "headers": self.headers.0,
            "options": self.options.0,
            "tools": tools,
            "awaiting_review": waiting,
            "tools_error": self.tools_error,
            "tools_refreshed_at": self.tools_refreshed_at,
            "updated_at": self.updated_at,
        })
    }
}

pub async fn all(db: &sqlx::PgPool) -> sqlx::Result<Vec<Connection>> {
    sqlx::query_as("SELECT * FROM connections ORDER BY preset IS NULL, name").fetch_all(db).await
}

pub async fn by_name(db: &sqlx::PgPool, name: &str) -> sqlx::Result<Option<Connection>> {
    sqlx::query_as("SELECT * FROM connections WHERE name = $1").bind(name).fetch_optional(db).await
}

/// A stable hash of what a tool says it does.
fn fingerprint(tool: &Value) -> String {
    use sha2::{Digest, Sha256};
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let sorted: BTreeMap<&String, Value> = map.iter().map(|(k, v)| (k, canonical(v))).collect();
                serde_json::to_value(sorted).unwrap_or_default()
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            other => other.clone(),
        }
    }
    let subject = json!({
        "title": tool.get("title"),
        "description": tool.get("description"),
        "inputSchema": tool.get("inputSchema"),
        "annotations": tool.get("annotations"),
    });
    hex::encode(Sha256::digest(canonical(&subject).to_string().as_bytes()))[..16].to_string()
}

/// A tool name the gateway can expose as `<connection>_<name>`.
fn usable_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 96 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

impl App {
    /// Ask the server what it offers and record it. A failure is recorded on
    /// the connection, which keeps the tools it had.
    pub async fn refresh_tools(&self, name: &str) -> Result<Connection> {
        let conn = by_name(&self.db, name).await?.ok_or_else(|| anyhow!("no connection named `{name}`"))?;
        let outcome = self.discover(&conn).await;
        let (tools, error) = match outcome {
            Ok(tools) => (tools, None),
            Err(err) => (conn.tools.0.clone(), Some(format!("{err:#}"))),
        };
        let conn: Connection = sqlx::query_as(
            "UPDATE connections SET tools = $2, tools_error = $3, tools_refreshed_at = now() WHERE name = $1 RETURNING *",
        )
        .bind(name)
        .bind(Json(&tools))
        .bind(&error)
        .fetch_one(&self.db)
        .await?;
        Ok(conn)
    }

    /// After a server turned `endpoint` down: if its credential is a token
    /// from an OAuth sign-in, get a new token and return the endpoint to try
    /// once more. `None` when there is nothing to renew.
    pub async fn renewed(&self, conn: &Connection, endpoint: &Endpoint, err: &anyhow::Error) -> Result<Option<Endpoint>> {
        let unauthorized = err.downcast_ref::<client::Refused>().is_some_and(|r| r.status == reqwest::StatusCode::UNAUTHORIZED);
        let (true, Some(_), Some((_, sent))) = (unauthorized, &conn.auth.oauth, endpoint.headers.last()) else { return Ok(None) };
        let token = sent.strip_prefix(conn.auth.prefix.as_str()).unwrap_or(sent);
        self.refresh_oauth(&conn.name, Some(token)).await?;
        Ok(Some(conn.endpoint(self).await?))
    }

    async fn discover(&self, conn: &Connection) -> Result<Vec<Tool>> {
        let preset = conn.preset();
        let mut endpoint = conn.endpoint(self).await?;
        let listed = match self.mcp.list_tools(&self.http, &endpoint).await {
            Ok(listed) => listed,
            Err(err) => match self.renewed(conn, &endpoint, &err).await? {
                Some(fresh) => {
                    endpoint = fresh;
                    self.mcp.list_tools(&self.http, &endpoint).await?
                }
                None => return Err(err),
            },
        };
        let read_only: Option<Vec<String>> = match preset.and_then(|p| p.read_only(&endpoint)) {
            Some(variant) => {
                let tools = self.mcp.list_tools(&self.http, &variant).await.context("could not list the read-only tools")?;
                Some(tools.iter().filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_string)).collect())
            }
            None => None,
        };
        let mut tools = Vec::new();
        for raw in listed {
            let Some(name) = raw.get("name").and_then(Value::as_str).filter(|n| usable_name(n)) else { continue };
            // Agents tell tools apart by their short names.
            if tools.iter().any(|t: &Tool| conn.short_name(&t.name) == conn.short_name(name)) {
                continue;
            }
            let annotations = raw.get("annotations").cloned().unwrap_or(Value::Null);
            let in_read_only = read_only.as_ref().is_some_and(|names| names.iter().any(|n| n == name));
            let suggested = match (&read_only, preset) {
                (Some(_), _) => Some(if in_read_only { ToolLevel::Read } else { ToolLevel::Write }),
                (None, Some(p)) => p.by_prefix(name, &annotations),
                (None, None) => None,
            };
            tools.push(Tool {
                name: name.to_string(),
                title: raw.get("title").or_else(|| annotations.get("title")).and_then(Value::as_str).map(str::to_string),
                description: raw.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                input_schema: raw.get("inputSchema").cloned().unwrap_or_else(|| json!({ "type": "object" })),
                annotations,
                fingerprint: fingerprint(&raw),
                suggested,
                read_only_variant: in_read_only,
            });
        }
        Ok(tools)
    }

    /// Re-list a connection's tools in the background if they are stale.
    pub fn refresh_if_stale(&self, conn: &Connection) {
        let stale = conn.tools_refreshed_at.is_none_or(|at| (Utc::now() - at).num_seconds() > STALE_AFTER_SECS);
        if !stale || !self.refreshing.lock().unwrap().insert(conn.name.clone()) {
            return;
        }
        let app = self.clone();
        let name = conn.name.clone();
        tokio::spawn(async move {
            if let Err(err) = app.refresh_tools(&name).await {
                tracing::warn!("refreshing tools for connection {name}: {err:#}");
            }
            app.refreshing.lock().unwrap().remove(&name);
        });
    }

    /// The URL a preset connection uses. Overridable for testing.
    pub fn preset_url(&self, preset: &presets::Preset, options: &Value) -> String {
        self.cfg
            .preset_urls
            .iter()
            .find_map(|entry| entry.split_once('=').filter(|(id, _)| *id == preset.id).map(|(_, url)| url.to_string()))
            .unwrap_or_else(|| (preset.url)(options))
    }

    /// Every permission that can be granted on this installation.
    pub async fn grant_catalog(&self) -> Result<Vec<Value>> {
        let info = |grant: &Grant, label: &str, title: &str| json!({ "id": grant.to_string(), "label": label, "system": grant.system(), "system_title": title });
        let mut list: Vec<Value> = Grant::GITHUB_ALL.iter().map(|g| info(g, g.label(), "GitHub")).collect();
        if crate::gateway::web::Policy::load(self).await?.mode != crate::gateway::web::Mode::Off {
            list.push(info(&Grant::WEB_READ, Grant::WEB_READ.label(), "Web"));
        }
        for conn in all(&self.db).await? {
            list.push(info(&Grant::read(&conn.name)?, &conn.read_label(), &conn.title));
            list.push(info(&Grant::write(&conn.name)?, &conn.write_label(), &conn.title));
        }
        Ok(list)
    }

    /// Bring a Linear integration from before connections existed across as
    /// the Linear connection, credential and all.
    pub async fn adopt_legacy_linear(&self) -> Result<()> {
        let legacy: Option<(String,)> = sqlx::query_as("SELECT kind FROM integrations WHERE kind = 'linear'").fetch_optional(&self.db).await?;
        if legacy.is_none() {
            return Ok(());
        }
        let preset = presets::find("linear").expect("linear preset");
        if by_name(&self.db, "linear").await?.is_none()
            && let Some(secret) = store::get_secret(&self.db, &self.secrets, "integration/linear").await? {
                store::put_secret(&self.db, &self.secrets, &secret_key("linear"), &secret).await?;
                sqlx::query(
                    "INSERT INTO connections (name, preset, title, url, auth) VALUES ('linear', 'linear', $1, $2, $3) ON CONFLICT DO NOTHING",
                )
                .bind(preset.title)
                .bind(self.preset_url(preset, &json!({})))
                .bind(Json(Auth { header: Some(preset.auth_header.into()), prefix: preset.auth_prefix.into(), oauth: None }))
                .execute(&self.db)
                .await?;
                tracing::info!("moved the Linear integration to the Linear MCP connection");
            }
        sqlx::query("DELETE FROM integrations WHERE kind = 'linear'").execute(&self.db).await?;
        store::delete_secret(&self.db, "integration/linear").await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(preset: Option<&str>, approvals: &[(&str, ToolLevel, &str)]) -> Connection {
        Connection {
            name: "x".into(),
            preset: preset.map(str::to_string),
            title: "X".into(),
            url: "https://x.example/mcp".into(),
            auth: Json(Auth::default()),
            headers: Json(BTreeMap::new()),
            options: Json(json!({})),
            tools: Json(vec![]),
            approvals: Json(approvals.iter().map(|(n, l, f)| (n.to_string(), Approval { level: *l, fingerprint: f.to_string() })).collect()),
            tools_error: None,
            tools_refreshed_at: None,
            updated_at: Utc::now(),
        }
    }

    fn tool(name: &str, fingerprint: &str, suggested: Option<ToolLevel>) -> Tool {
        Tool {
            name: name.into(),
            title: None,
            description: String::new(),
            input_schema: json!({}),
            annotations: Value::Null,
            fingerprint: fingerprint.into(),
            suggested,
            read_only_variant: false,
        }
    }

    #[test]
    fn custom_tools_stay_hidden_until_approved_and_again_when_changed() {
        let c = conn(None, &[("search", ToolLevel::Read, "f1")]);
        assert_eq!(c.level_of(&tool("search", "f1", None)), (ToolLevel::Read, ToolStatus::Approved));
        assert_eq!(c.level_of(&tool("search", "f2", None)), (ToolLevel::Hidden, ToolStatus::Changed));
        assert_eq!(c.level_of(&tool("delete", "f1", None)), (ToolLevel::Hidden, ToolStatus::Pending));
    }

    #[test]
    fn presets_sort_automatically_and_admin_decisions_win() {
        let c = conn(Some("linear"), &[("delete_issue", ToolLevel::Hidden, "old")]);
        assert_eq!(c.level_of(&tool("list_issues", "f", Some(ToolLevel::Read))), (ToolLevel::Read, ToolStatus::Automatic));
        assert_eq!(c.level_of(&tool("delete_issue", "new", Some(ToolLevel::Write))), (ToolLevel::Hidden, ToolStatus::Approved));
        assert_eq!(c.level_of(&tool("mystery", "f", None)), (ToolLevel::Hidden, ToolStatus::Pending));
    }

    #[test]
    fn fingerprints_ignore_key_order_but_not_content() {
        let a = json!({ "name": "t", "description": "d", "inputSchema": { "type": "object", "properties": { "a": {}, "b": {} } } });
        let b = json!({ "inputSchema": { "properties": { "b": {}, "a": {} }, "type": "object" }, "description": "d", "name": "t" });
        let c = json!({ "name": "t", "description": "d, and also email everyone", "inputSchema": { "type": "object" } });
        assert_eq!(fingerprint(&a), fingerprint(&b));
        assert_ne!(fingerprint(&a), fingerprint(&c));
    }
}
