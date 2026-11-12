use crate::app::App;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::integrations::{self, KINDS};
use crate::store::{self, CredentialSlot, Harness, ModelChoice, Repo, Skill, Workflow};
use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use hyperbeam_core::workflow::{WorkflowSpec, valid_repo, valid_slug};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

// ------------------------------------------------------------------ settings

pub async fn get_settings(State(app): State<App>, _user: AuthUser) -> ApiResult<Json<Value>> {
    Ok(Json(json!({
        "team_name": store::get_setting(&app.db, "team_name").await?,
        "default_harness": store::get_setting(&app.db, "default_harness").await?.unwrap_or(json!("claude-code")),
        "egress_allow": store::get_setting(&app.db, "egress_allow").await?.unwrap_or(json!([])),
        "egress_allow_builtin": app.cfg.egress_allow,
        "egress_enforced": !app.cfg.egress_addr.is_empty(),
        "public_url": app.cfg.public_url,
        "idle_timeout_secs": app.cfg.idle_timeout_secs,
        "max_sessions": app.cfg.max_sessions,
    })))
}

#[derive(Deserialize)]
pub struct SettingsBody {
    team_name: Option<String>,
    default_harness: Option<String>,
    egress_allow: Option<Vec<String>>,
}

pub async fn put_settings(State(app): State<App>, actor: AuthUser, Json(body): Json<SettingsBody>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    if let Some(name) = body.team_name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        store::set_setting(&app.db, "team_name", &json!(name)).await?;
    }
    if let Some(harness) = &body.default_harness {
        store::harness_by_name(&app.db, harness).await?.ok_or_else(|| ApiError::bad(format!("no harness profile named `{harness}`")))?;
        store::set_setting(&app.db, "default_harness", &json!(harness)).await?;
    }
    if let Some(hosts) = &body.egress_allow {
        let hosts = clean_hosts(hosts)?;
        store::set_setting(&app.db, "egress_allow", &json!(hosts)).await?;
    }
    get_settings(State(app), actor).await
}

fn clean_hosts(hosts: &[String]) -> ApiResult<Vec<String>> {
    let mut out = Vec::new();
    for host in hosts {
        let host = host.trim().to_lowercase();
        if host.is_empty() {
            continue;
        }
        let name = host.strip_prefix("*.").unwrap_or(&host);
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') || !name.contains('.') {
            return Err(ApiError::bad(format!("`{host}` is not a hostname (use example.com or *.example.com)")));
        }
        out.push(host);
    }
    Ok(out)
}

// -------------------------------------------------------------- integrations

pub async fn list_integrations(State(app): State<App>, _user: AuthUser) -> ApiResult<Json<Value>> {
    let rows: Vec<(String, Value, DateTime<Utc>)> = sqlx::query_as("SELECT kind, config, updated_at FROM integrations").fetch_all(&app.db).await?;
    let list: Vec<Value> = KINDS
        .iter()
        .map(|kind| match rows.iter().find(|(k, _, _)| k == kind) {
            Some((_, config, updated_at)) => json!({ "kind": kind, "connected": true, "config": config, "updated_at": updated_at }),
            None => json!({ "kind": kind, "connected": false, "config": {} }),
        })
        .collect();
    Ok(Json(json!({ "integrations": list })))
}

#[derive(Deserialize)]
pub struct IntegrationBody {
    #[serde(default)]
    config: Value,
    /// Token, API key or GitHub App private key. Optional when updating.
    secret: Option<String>,
}

pub async fn put_integration(
    State(app): State<App>,
    actor: AuthUser,
    Path(kind): Path<String>,
    Json(body): Json<IntegrationBody>,
) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    if !KINDS.contains(&kind.as_str()) {
        return Err(ApiError::NotFound("integration"));
    }
    let config = match kind.as_str() {
        "github" => {
            let mode = body.config.get("mode").and_then(Value::as_str).unwrap_or("token");
            match mode {
                "token" => json!({ "mode": "token" }),
                "app" => {
                    let app_id = match body.config.get("app_id") {
                        Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
                        Some(Value::Number(n)) => n.to_string(),
                        _ => return Err(ApiError::bad("a GitHub App needs its App ID")),
                    };
                    json!({ "mode": "app", "app_id": app_id })
                }
                _ => return Err(ApiError::bad("GitHub mode must be `token` or `app`")),
            }
        }
        _ => json!({}),
    };
    let key = integrations::secret_key(&kind);
    match body.secret.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(secret) => {
            if config["mode"] == "app" && !secret.contains("PRIVATE KEY") {
                return Err(ApiError::bad("paste the GitHub App's private key (PEM)"));
            }
            store::put_secret(&app.db, &app.secrets, &key, secret).await?;
        }
        None => {
            if store::get_secret(&app.db, &app.secrets, &key).await?.is_none() {
                return Err(ApiError::bad("a credential is required"));
            }
        }
    }
    sqlx::query(
        "INSERT INTO integrations (kind, config, updated_by) VALUES ($1, $2, $3)
         ON CONFLICT (kind) DO UPDATE SET config = excluded.config, updated_by = excluded.updated_by, updated_at = now()",
    )
    .bind(&kind)
    .bind(&config)
    .bind(actor.0.id)
    .execute(&app.db)
    .await?;
    app.github_tokens.clear();
    app.activity(Some(actor.0.id), None, None, "integration.connected", &format!("{} connected {kind}", actor.0.name), json!({})).await;
    Ok(Json(json!({ "kind": kind, "connected": true, "config": config })))
}

pub async fn delete_integration(State(app): State<App>, actor: AuthUser, Path(kind): Path<String>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    sqlx::query("DELETE FROM integrations WHERE kind = $1").bind(&kind).execute(&app.db).await?;
    store::delete_secret(&app.db, &integrations::secret_key(&kind)).await?;
    app.github_tokens.clear();
    Ok(Json(json!({ "ok": true })))
}

/// Make one cheap authenticated call to confirm the credential works.
pub async fn test_integration(State(app): State<App>, actor: AuthUser, Path(kind): Path<String>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let secret = app.integration_secret(&kind).await?.ok_or_else(|| ApiError::bad(format!("{kind} is not connected")))?;
    let result: anyhow::Result<String> = async {
        match kind.as_str() {
            "github" => {
                let config = app.integration_config("github").await?.unwrap_or_default();
                let api = app.cfg.github_api_url.trim_end_matches('/');
                if config["mode"] == "app" {
                    // Minting a token for a registered repository exercises the whole chain.
                    let repo: Option<(String,)> = sqlx::query_as("SELECT full_name FROM repos ORDER BY created_at LIMIT 1").fetch_optional(&app.db).await?;
                    match repo {
                        Some((repo,)) => {
                            app.github_token(&repo, crate::integrations::github::Access::Read).await?;
                            Ok(format!("GitHub App can read {repo}"))
                        }
                        None => Ok("Saved. Register a repository to verify the App is installed on it.".to_string()),
                    }
                } else {
                    let resp = app.http.get(format!("{api}/user")).bearer_auth(&secret).header("accept", "application/vnd.github+json").send().await?;
                    if !resp.status().is_success() {
                        anyhow::bail!("GitHub rejected the token ({})", resp.status());
                    }
                    let user: Value = resp.json().await?;
                    Ok(format!("Authenticated to GitHub as {}", user["login"].as_str().unwrap_or("unknown")))
                }
            }
            _ => anyhow::bail!("unknown integration"),
        }
    }
    .await;
    Ok(Json(match result {
        Ok(detail) => json!({ "ok": true, "detail": detail }),
        Err(err) => json!({ "ok": false, "detail": format!("{err:#}") }),
    }))
}

// --------------------------------------------------------------------- repos

pub async fn list_repos(State(app): State<App>, AuthUser(user): AuthUser) -> ApiResult<Json<Value>> {
    let repos: Vec<Repo> = sqlx::query_as("SELECT * FROM repos ORDER BY full_name").fetch_all(&app.db).await?;
    let scope = user.repo_scope();
    let list: Vec<Value> = repos
        .into_iter()
        .map(|repo| {
            let allowed = scope.allows(&repo.full_name);
            let mut value = serde_json::to_value(&repo).unwrap_or_default();
            value["accessible"] = json!(allowed);
            value
        })
        .collect();
    Ok(Json(json!({ "repos": list })))
}

#[derive(Deserialize)]
pub struct NewRepo {
    full_name: String,
    default_branch: Option<String>,
}

pub async fn add_repo(State(app): State<App>, actor: AuthUser, Json(body): Json<NewRepo>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let name = body
        .full_name
        .trim()
        .trim_start_matches("https://github.com/")
        .trim_end_matches(".git")
        .trim_matches('/')
        .to_string();
    if !valid_repo(&name) {
        return Err(ApiError::bad("write the repository as owner/name"));
    }
    let mut full_name = name.clone();
    let mut default_branch = body.default_branch.clone().filter(|b| !b.trim().is_empty());
    let mut description = String::new();
    // When GitHub is connected, confirm access and learn the default branch.
    if app.integration_config("github").await?.is_some() {
        match app.github_api(reqwest::Method::GET, &name, "", crate::integrations::github::Access::Read, None).await {
            Ok(info) => {
                full_name = info["full_name"].as_str().unwrap_or(&name).to_string();
                default_branch = default_branch.or_else(|| info["default_branch"].as_str().map(str::to_string));
                description = info["description"].as_str().unwrap_or("").to_string();
            }
            Err(err) => return Err(ApiError::bad(format!("could not read {name} with the connected GitHub credential: {err:#}"))),
        }
    }
    let repo: Repo = sqlx::query_as(
        "INSERT INTO repos (id, full_name, default_branch, description, added_by) VALUES ($1, $2, $3, $4, $5) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(&full_name)
    .bind(default_branch.unwrap_or_else(|| "main".into()))
    .bind(description)
    .bind(actor.0.id)
    .fetch_one(&app.db)
    .await
    .map_err(|err| match ApiError::from(err) {
        ApiError::Conflict(_) => ApiError::Conflict(format!("{full_name} is already registered")),
        other => other,
    })?;
    Ok(Json(json!({ "repo": repo })))
}

pub async fn delete_repo(State(app): State<App>, actor: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    sqlx::query("DELETE FROM repos WHERE id = $1").bind(id).execute(&app.db).await?;
    Ok(Json(json!({ "ok": true })))
}

// ----------------------------------------------------------------- harnesses

pub async fn list_harnesses(State(app): State<App>, AuthUser(user): AuthUser) -> ApiResult<Json<Value>> {
    let harnesses: Vec<Harness> = sqlx::query_as("SELECT * FROM harnesses ORDER BY builtin DESC, name").fetch_all(&app.db).await?;
    let creds: Vec<(Uuid, Uuid, String)> =
        sqlx::query_as("SELECT harness_id, user_id, env FROM harness_credentials WHERE user_id IN ($1, $2)")
            .bind(user.id)
            .bind(Uuid::nil())
            .fetch_all(&app.db)
            .await?;
    let list: Vec<Value> = harnesses
        .into_iter()
        .map(|h| {
            let find = |owner: Uuid| creds.iter().find(|(hid, uid, _)| *hid == h.id && *uid == owner).map(|(_, _, env)| env.clone());
            let team = find(Uuid::nil());
            let mine = find(user.id);
            let ready = h.credentials.is_empty() || team.is_some() || mine.is_some();
            let mut value = serde_json::to_value(&h).unwrap_or_default();
            value["team_credential"] = json!(team);
            value["my_credential"] = json!(mine);
            value["ready"] = json!(ready);
            // What to offer when starting work: the curated list, else what the harness reported.
            value["model_choices"] = json!(h.choices());
            // Model discovery in flight, and how the last attempt went wrong.
            let (refreshing, problem) = app.probes.status(h.id);
            value["models_refreshing"] = json!(refreshing);
            value["models_error"] = json!(problem);
            value
        })
        .collect();
    Ok(Json(json!({ "harnesses": list })))
}

#[derive(Deserialize)]
pub struct HarnessBody {
    name: Option<String>,
    title: Option<String>,
    description: Option<String>,
    image: Option<String>,
    command: Option<Vec<String>>,
    env: Option<BTreeMap<String, String>>,
    credentials: Option<Vec<CredentialSlot>>,
    egress_hosts: Option<Vec<String>>,
    /// Restrict the models people may pick. Empty lifts the restriction.
    models: Option<Vec<ModelChoice>>,
    /// `null` or empty returns to the harness's own default.
    #[serde(default, deserialize_with = "super::nullable")]
    default_model: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::nullable")]
    model_env: Option<Option<String>>,
}

fn clean_models(models: Vec<ModelChoice>) -> ApiResult<Vec<ModelChoice>> {
    let mut out: Vec<ModelChoice> = Vec::new();
    for mut model in models {
        model.id = model.id.trim().to_string();
        model.label = model.label.trim().to_string();
        if model.id.is_empty() || model.id.len() > 200 || model.id.contains(char::is_whitespace) {
            return Err(ApiError::bad(format!("`{}` is not a model id", model.id)));
        }
        if !out.iter().any(|m| m.id == model.id) {
            out.push(model);
        }
    }
    Ok(out)
}

/// Normalise the model settings of a profile and check they agree.
fn model_settings(
    models: Vec<ModelChoice>,
    default_model: Option<String>,
    model_env: Option<String>,
) -> ApiResult<(Vec<ModelChoice>, Option<String>, Option<String>)> {
    let models = clean_models(models)?;
    let default_model = default_model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
    let model_env = model_env.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
    if let Some(default) = &default_model
        && !models.is_empty() && !models.iter().any(|m| &m.id == default) {
            return Err(ApiError::bad(format!("the default model `{default}` is not in the allowed list")));
        }
    if let Some(var) = &model_env
        && !valid_env_name(var) {
            return Err(ApiError::bad(format!("`{var}` is not a valid environment variable name")));
        }
    Ok((models, default_model, model_env))
}

fn check_slots(slots: &[CredentialSlot]) -> ApiResult<()> {
    for slot in slots {
        if !valid_env_name(&slot.env) {
            return Err(ApiError::bad(format!("`{}` is not a valid environment variable name", slot.env)));
        }
        if !["api_key", "oauth"].contains(&slot.kind.as_str()) {
            return Err(ApiError::bad("credential kind must be `api_key` or `oauth`"));
        }
    }
    Ok(())
}

fn valid_env_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
        // The runner's own wiring is not for profiles to override.
        && !name.starts_with("HYPERBEAM_")
        && !name.starts_with("GIT_CONFIG_")
}

pub async fn create_harness(State(app): State<App>, actor: AuthUser, Json(body): Json<HarnessBody>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let name = body.name.as_deref().map(str::trim).unwrap_or_default().to_string();
    if !valid_slug(&name) {
        return Err(ApiError::bad("the profile name must be lowercase letters, digits and dashes"));
    }
    let image = body.image.as_deref().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| ApiError::bad("a container image is required"))?;
    let command = body.command.clone().filter(|c| !c.is_empty()).ok_or_else(|| ApiError::bad("the command that starts the ACP agent is required"))?;
    let credentials = body.credentials.clone().unwrap_or_default();
    check_slots(&credentials)?;
    let env = body.env.clone().unwrap_or_default();
    if let Some(bad) = env.keys().find(|k| !valid_env_name(k)) {
        return Err(ApiError::bad(format!("`{bad}` is not an environment variable a profile may set")));
    }
    let (models, default_model, model_env) = model_settings(
        body.models.clone().unwrap_or_default(),
        body.default_model.clone().flatten(),
        body.model_env.clone().flatten(),
    )?;
    let harness: Harness = sqlx::query_as(
        "INSERT INTO harnesses (id, name, title, description, image, command, env, credentials, egress_hosts,
                                models, default_model, model_env)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(&name)
    .bind(body.title.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&name))
    .bind(body.description.as_deref().unwrap_or(""))
    .bind(image)
    .bind(json!(command))
    .bind(json!(env))
    .bind(json!(credentials))
    .bind(json!(clean_hosts(&body.egress_hosts.clone().unwrap_or_default())?))
    .bind(json!(models))
    .bind(default_model)
    .bind(model_env)
    .fetch_one(&app.db)
    .await?;
    app.discover_models(harness.id, actor.0.id).await;
    Ok(Json(json!({ "harness": harness })))
}

pub async fn update_harness(
    State(app): State<App>,
    actor: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<HarnessBody>,
) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let current = store::harness_by_id(&app.db, id).await?.ok_or(ApiError::NotFound("harness"))?;
    let previous = (current.image.clone(), current.command.0.clone(), current.env.0.clone());
    let credentials = body.credentials.clone().unwrap_or(current.credentials.0);
    check_slots(&credentials)?;
    let env = body.env.clone().unwrap_or(current.env.0);
    if let Some(bad) = env.keys().find(|k| !valid_env_name(k)) {
        return Err(ApiError::bad(format!("`{bad}` is not an environment variable a profile may set")));
    }
    let command = body.command.clone().unwrap_or(current.command.0);
    if command.is_empty() {
        return Err(ApiError::bad("the command cannot be empty"));
    }
    let egress = match &body.egress_hosts {
        Some(hosts) => clean_hosts(hosts)?,
        None => current.egress_hosts.0,
    };
    let (models, default_model, model_env) = model_settings(
        body.models.clone().unwrap_or(current.models.0),
        body.default_model.clone().unwrap_or(current.default_model),
        body.model_env.clone().unwrap_or(current.model_env),
    )?;
    let harness: Harness = sqlx::query_as(
        "UPDATE harnesses SET title = $2, description = $3, image = $4, command = $5, env = $6, credentials = $7, egress_hosts = $8,
                              models = $9, default_model = $10, model_env = $11
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(body.title.unwrap_or(current.title))
    .bind(body.description.unwrap_or(current.description))
    .bind(body.image.map(|i| i.trim().to_string()).filter(|i| !i.is_empty()).unwrap_or(current.image))
    .bind(json!(command))
    .bind(json!(env))
    .bind(json!(credentials))
    .bind(json!(egress))
    .bind(json!(models))
    .bind(default_model)
    .bind(model_env)
    .fetch_one(&app.db)
    .await?;
    if previous != (harness.image.clone(), harness.command.0.clone(), harness.env.0.clone()) {
        // A different agent may offer different models.
        app.discover_models(harness.id, actor.0.id).await;
    }
    Ok(Json(json!({ "harness": harness })))
}

pub async fn delete_harness(State(app): State<App>, actor: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let harness = store::harness_by_id(&app.db, id).await?.ok_or(ApiError::NotFound("harness"))?;
    if harness.builtin {
        return Err(ApiError::bad("built-in profiles cannot be deleted (they can be edited)"));
    }
    let (used,): (i64,) = sqlx::query_as("SELECT count(*) FROM sessions WHERE harness_id = $1").bind(id).fetch_one(&app.db).await?;
    if used > 0 {
        return Err(ApiError::Conflict(format!("{used} session(s) used this profile; it is part of their history")));
    }
    let owners: Vec<(Uuid,)> = sqlx::query_as("SELECT user_id FROM harness_credentials WHERE harness_id = $1").bind(id).fetch_all(&app.db).await?;
    for (owner,) in owners {
        store::delete_secret(&app.db, &format!("harness/{id}/{owner}")).await?;
    }
    sqlx::query("DELETE FROM harnesses WHERE id = $1").bind(id).execute(&app.db).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct CredentialBody {
    /// `team` (admins) or `me`.
    scope: String,
    env: String,
    value: String,
}

pub async fn put_credential(
    State(app): State<App>,
    actor: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<CredentialBody>,
) -> ApiResult<Json<Value>> {
    let harness = store::harness_by_id(&app.db, id).await?.ok_or(ApiError::NotFound("harness"))?;
    let slot = harness
        .credentials
        .iter()
        .find(|slot| slot.env == body.env)
        .ok_or_else(|| ApiError::bad(format!("{} does not accept a credential named {}", harness.title, body.env)))?;
    let owner = match body.scope.as_str() {
        "team" => {
            actor.require_admin()?;
            if slot.kind == "oauth" {
                // A subscription belongs to one person; sharing it across a
                // team is against providers' terms.
                return Err(ApiError::bad("subscription tokens are personal: each person adds their own under “My credential”"));
            }
            Uuid::nil()
        }
        "me" => actor.0.id,
        _ => return Err(ApiError::bad("scope must be `team` or `me`")),
    };
    let value = body.value.trim();
    if value.is_empty() {
        return Err(ApiError::bad("the credential is empty"));
    }
    store::put_secret(&app.db, &app.secrets, &format!("harness/{id}/{owner}"), value).await?;
    sqlx::query(
        "INSERT INTO harness_credentials (harness_id, user_id, env) VALUES ($1, $2, $3)
         ON CONFLICT (harness_id, user_id) DO UPDATE SET env = excluded.env, updated_at = now()",
    )
    .bind(id)
    .bind(owner)
    .bind(&body.env)
    .execute(&app.db)
    .await?;
    // Now that the harness can start, ask it which models it offers so the
    // list is there before anyone's first session.
    let refreshing = app.discover_models(id, actor.0.id).await;
    Ok(Json(json!({ "ok": true, "models_refreshing": refreshing })))
}

/// Ask the harness again which models it offers.
pub async fn refresh_models(State(app): State<App>, actor: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let harness = store::harness_by_id(&app.db, id).await?.ok_or(ApiError::NotFound("harness"))?;
    if !app.discover_models(id, actor.0.id).await {
        return Err(ApiError::bad(format!("{} needs a credential before it can be asked for its models", harness.title)));
    }
    Ok(Json(json!({ "ok": true, "models_refreshing": true })))
}

#[derive(Deserialize)]
pub struct ScopeQuery {
    scope: String,
}

pub async fn delete_credential(
    State(app): State<App>,
    actor: AuthUser,
    Path(id): Path<Uuid>,
    Query(query): Query<ScopeQuery>,
) -> ApiResult<Json<Value>> {
    let owner = match query.scope.as_str() {
        "team" => {
            actor.require_admin()?;
            Uuid::nil()
        }
        "me" => actor.0.id,
        _ => return Err(ApiError::bad("scope must be `team` or `me`")),
    };
    sqlx::query("DELETE FROM harness_credentials WHERE harness_id = $1 AND user_id = $2").bind(id).bind(owner).execute(&app.db).await?;
    store::delete_secret(&app.db, &format!("harness/{id}/{owner}")).await?;
    Ok(Json(json!({ "ok": true })))
}

// -------------------------------------------------------------------- skills

pub async fn list_skills(State(app): State<App>, _user: AuthUser) -> ApiResult<Json<Value>> {
    let skills: Vec<Skill> = sqlx::query_as("SELECT id, name, description, content, updated_at FROM skills ORDER BY name").fetch_all(&app.db).await?;
    Ok(Json(json!({ "skills": skills })))
}

#[derive(Deserialize)]
pub struct SkillBody {
    name: String,
    #[serde(default)]
    description: String,
    content: String,
}

fn check_skill(body: &SkillBody) -> ApiResult<()> {
    if !valid_slug(body.name.trim()) {
        return Err(ApiError::bad("the skill name must be lowercase letters, digits and dashes"));
    }
    if body.content.trim().is_empty() {
        return Err(ApiError::bad("a skill needs instructions"));
    }
    if body.content.len() > 200_000 {
        return Err(ApiError::bad("the skill is too large (limit 200 KB)"));
    }
    Ok(())
}

pub async fn create_skill(State(app): State<App>, actor: AuthUser, Json(body): Json<SkillBody>) -> ApiResult<Json<Value>> {
    check_skill(&body)?;
    let skill: Skill = sqlx::query_as(
        "INSERT INTO skills (id, name, description, content, updated_by) VALUES ($1, $2, $3, $4, $5)
         RETURNING id, name, description, content, updated_at",
    )
    .bind(Uuid::now_v7())
    .bind(body.name.trim())
    .bind(body.description.trim())
    .bind(&body.content)
    .bind(actor.0.id)
    .fetch_one(&app.db)
    .await?;
    Ok(Json(json!({ "skill": skill })))
}

pub async fn update_skill(State(app): State<App>, actor: AuthUser, Path(id): Path<Uuid>, Json(body): Json<SkillBody>) -> ApiResult<Json<Value>> {
    check_skill(&body)?;
    let skill: Option<Skill> = sqlx::query_as(
        "UPDATE skills SET name = $2, description = $3, content = $4, updated_by = $5, updated_at = now() WHERE id = $1
         RETURNING id, name, description, content, updated_at",
    )
    .bind(id)
    .bind(body.name.trim())
    .bind(body.description.trim())
    .bind(&body.content)
    .bind(actor.0.id)
    .fetch_optional(&app.db)
    .await?;
    Ok(Json(json!({ "skill": skill.ok_or(ApiError::NotFound("skill"))? })))
}

pub async fn delete_skill(State(app): State<App>, _actor: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    sqlx::query("DELETE FROM skills WHERE id = $1").bind(id).execute(&app.db).await?;
    Ok(Json(json!({ "ok": true })))
}

// ----------------------------------------------------------------- workflows

pub async fn list_workflows(State(app): State<App>, _user: AuthUser) -> ApiResult<Json<Value>> {
    let workflows: Vec<Workflow> = sqlx::query_as("SELECT id, name, description, yaml, updated_at FROM workflows ORDER BY name").fetch_all(&app.db).await?;
    let list: Vec<Value> = workflows
        .into_iter()
        .map(|wf| {
            let spec = WorkflowSpec::from_yaml(&wf.yaml).ok();
            let mut value = serde_json::to_value(&wf).unwrap_or_default();
            value["spec"] = json!(spec);
            value
        })
        .collect();
    Ok(Json(json!({ "workflows": list })))
}

#[derive(Deserialize)]
pub struct WorkflowBody {
    yaml: String,
}

pub async fn create_workflow(State(app): State<App>, actor: AuthUser, Json(body): Json<WorkflowBody>) -> ApiResult<Json<Value>> {
    let spec = WorkflowSpec::from_yaml(&body.yaml).map_err(|e| ApiError::bad(e.to_string()))?;
    let workflow: Workflow = sqlx::query_as(
        "INSERT INTO workflows (id, name, description, yaml, updated_by) VALUES ($1, $2, $3, $4, $5)
         RETURNING id, name, description, yaml, updated_at",
    )
    .bind(Uuid::now_v7())
    .bind(&spec.name)
    .bind(&spec.description)
    .bind(&body.yaml)
    .bind(actor.0.id)
    .fetch_one(&app.db)
    .await?;
    Ok(Json(json!({ "workflow": workflow })))
}

pub async fn update_workflow(State(app): State<App>, actor: AuthUser, Path(id): Path<Uuid>, Json(body): Json<WorkflowBody>) -> ApiResult<Json<Value>> {
    let spec = WorkflowSpec::from_yaml(&body.yaml).map_err(|e| ApiError::bad(e.to_string()))?;
    let workflow: Option<Workflow> = sqlx::query_as(
        "UPDATE workflows SET name = $2, description = $3, yaml = $4, updated_by = $5, updated_at = now() WHERE id = $1
         RETURNING id, name, description, yaml, updated_at",
    )
    .bind(id)
    .bind(&spec.name)
    .bind(&spec.description)
    .bind(&body.yaml)
    .bind(actor.0.id)
    .fetch_optional(&app.db)
    .await?;
    Ok(Json(json!({ "workflow": workflow.ok_or(ApiError::NotFound("workflow"))? })))
}

pub async fn delete_workflow(State(app): State<App>, _actor: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    sqlx::query("DELETE FROM workflows WHERE id = $1").bind(id).execute(&app.db).await?;
    Ok(Json(json!({ "ok": true })))
}

// -------------------------------------------------------------------- memory

pub async fn list_memory(State(app): State<App>, _user: AuthUser) -> ApiResult<Json<Value>> {
    let rows: Vec<(String, String, DateTime<Utc>, Option<Uuid>, Option<Uuid>)> =
        sqlx::query_as("SELECT key, content, updated_at, updated_by_user, updated_by_session FROM memory ORDER BY key").fetch_all(&app.db).await?;
    let notes: Vec<Value> = rows
        .into_iter()
        .map(|(key, content, updated_at, user, session)| {
            json!({ "key": key, "content": content, "updated_at": updated_at, "updated_by_user": user, "updated_by_session": session })
        })
        .collect();
    Ok(Json(json!({ "notes": notes })))
}

#[derive(Deserialize)]
pub struct MemoryBody {
    content: String,
}

pub async fn put_memory(State(app): State<App>, actor: AuthUser, Path(key): Path<String>, Json(body): Json<MemoryBody>) -> ApiResult<Json<Value>> {
    if !valid_slug(&key) {
        return Err(ApiError::bad("the note key must be lowercase letters, digits and dashes"));
    }
    if body.content.len() > 64 * 1024 {
        return Err(ApiError::bad("the note is too large (limit 64 KB)"));
    }
    sqlx::query(
        "INSERT INTO memory (key, content, updated_by_user) VALUES ($1, $2, $3)
         ON CONFLICT (key) DO UPDATE SET content = excluded.content, updated_by_user = excluded.updated_by_user,
                                         updated_by_session = NULL, updated_at = now()",
    )
    .bind(&key)
    .bind(&body.content)
    .bind(actor.0.id)
    .execute(&app.db)
    .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_memory(State(app): State<App>, _actor: AuthUser, Path(key): Path<String>) -> ApiResult<Json<Value>> {
    sqlx::query("DELETE FROM memory WHERE key = $1").bind(&key).execute(&app.db).await?;
    Ok(Json(json!({ "ok": true })))
}
