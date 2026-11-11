//! MCP connections: first-class (preset) and custom.

use crate::app::App;
use crate::auth::AuthUser;
use crate::connections::{self, Approval, Auth, Connection, RESERVED, ToolLevel, presets};
use crate::error::{ApiError, ApiResult};
use crate::store;
use axum::Json;
use axum::extract::{Path, State};
use hyperbeam_core::GrantSet;
use hyperbeam_core::grants::valid_system;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::types::Json as SqlJson;
use std::collections::BTreeMap;

pub async fn list(State(app): State<App>, _user: AuthUser) -> ApiResult<Json<Value>> {
    let conns = connections::all(&app.db).await?;
    let presets: Vec<Value> = presets::PRESETS
        .iter()
        .map(|p| {
            let mut value = p.describe();
            value["connected"] = json!(conns.iter().any(|c| c.preset.as_deref() == Some(p.id)));
            value
        })
        .collect();
    Ok(Json(json!({ "presets": presets, "connections": conns.iter().map(Connection::view).collect::<Vec<_>>() })))
}

/// Every permission that can be granted here, for the people screen.
pub async fn grants(State(app): State<App>, _user: AuthUser) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "grants": app.grant_catalog().await? })))
}

#[derive(Deserialize)]
pub struct AuthBody {
    /// `bearer`, `header` or `none`.
    kind: String,
    #[serde(default)]
    header: Option<String>,
}

#[derive(Deserialize)]
pub struct ConnectionBody {
    /// Custom connections only; a preset's name is its own.
    name: Option<String>,
    preset: Option<String>,
    title: Option<String>,
    url: Option<String>,
    auth: Option<AuthBody>,
    headers: Option<BTreeMap<String, String>>,
    options: Option<Value>,
    /// Optional when updating.
    secret: Option<String>,
}

/// Headers the gateway sets itself, or that would break the request.
const OWN_HEADERS: &[&str] = &["accept", "content-type", "content-length", "host", "connection", "transfer-encoding", "mcp-session-id", "mcp-protocol-version"];

fn check_header(name: &str) -> ApiResult<String> {
    let name = name.trim();
    let token = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c));
    if !token {
        return Err(ApiError::bad(format!("`{name}` is not a header name")));
    }
    if OWN_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
        return Err(ApiError::bad(format!("Hyperbeam sets `{name}` itself")));
    }
    Ok(name.to_string())
}

fn clean_headers(headers: &BTreeMap<String, String>, auth: &Auth) -> ApiResult<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (name, value) in headers {
        let name = check_header(name)?;
        if auth.header.as_deref().is_some_and(|h| h.eq_ignore_ascii_case(&name)) {
            return Err(ApiError::bad(format!("`{name}` already carries the credential")));
        }
        if value.contains(['\r', '\n']) || value.len() > 4096 {
            return Err(ApiError::bad(format!("the value of `{name}` is not a valid header value")));
        }
        out.insert(name, value.trim().to_string());
    }
    Ok(out)
}

fn check_url(raw: &str) -> ApiResult<String> {
    let url = url::Url::parse(raw.trim()).map_err(|_| ApiError::bad(format!("`{raw}` is not a URL")))?;
    if !matches!(url.scheme(), "https" | "http") || url.host_str().is_none() {
        return Err(ApiError::bad("the server URL must start with https:// (or http:// for a server inside your network)"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ApiError::bad("put credentials in the credential field, not the URL"));
    }
    Ok(url.to_string())
}

fn custom_auth(body: &AuthBody) -> ApiResult<Auth> {
    match body.kind.as_str() {
        "bearer" => Ok(Auth { header: Some("Authorization".into()), prefix: "Bearer ".into() }),
        "header" => {
            let header = check_header(body.header.as_deref().unwrap_or_default())?;
            Ok(Auth { header: Some(header), prefix: String::new() })
        }
        "none" => Ok(Auth::default()),
        _ => Err(ApiError::bad("auth must be `bearer`, `header` or `none`")),
    }
}

fn clean_options(preset: &presets::Preset, options: &Value) -> Value {
    let allowed = (preset.options)();
    let mut out = serde_json::Map::new();
    for option in allowed.as_array().into_iter().flatten() {
        let Some(key) = option["key"].as_str() else { continue };
        if let Some(value) = options.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()) {
            out.insert(key.to_string(), json!(value));
        }
    }
    Value::Object(out)
}

async fn save_secret(app: &App, name: &str, auth: &Auth, secret: Option<&str>, required: bool) -> ApiResult<()> {
    let key = connections::secret_key(name);
    match (auth.header.is_some(), secret.map(str::trim).filter(|s| !s.is_empty())) {
        (false, _) => store::delete_secret(&app.db, &key).await?,
        (true, Some(secret)) => {
            if secret.contains(['\r', '\n']) {
                return Err(ApiError::bad("the credential must be on one line"));
            }
            store::put_secret(&app.db, &app.secrets, &key, secret).await?;
        }
        (true, None) if required || store::get_secret(&app.db, &app.secrets, &key).await?.is_none() => {
            return Err(ApiError::bad("a credential is required"));
        }
        (true, None) => {}
    }
    Ok(())
}

/// After saving: forget old upstream sessions, list the tools, report.
async fn saved(app: &App, name: &str) -> ApiResult<Json<Value>> {
    app.mcp.reset();
    let conn = app.refresh_tools(name).await?;
    Ok(Json(json!({ "connection": conn.view() })))
}

pub async fn create(State(app): State<App>, actor: AuthUser, Json(body): Json<ConnectionBody>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let (name, preset, title, url, auth, options) = match body.preset.as_deref() {
        Some(id) => {
            let preset = presets::find(id).ok_or_else(|| ApiError::bad(format!("no first-class connection called `{id}`")))?;
            let options = clean_options(preset, body.options.as_ref().unwrap_or(&Value::Null));
            let url = app.preset_url(preset, &options);
            let auth = Auth { header: Some(preset.auth_header.into()), prefix: preset.auth_prefix.into() };
            (preset.id.to_string(), Some(preset.id), preset.title.to_string(), url, auth, options)
        }
        None => {
            let name = body.name.as_deref().map(str::trim).unwrap_or_default().to_string();
            if !valid_system(&name) {
                return Err(ApiError::bad("the name must be lowercase letters, digits and dashes, starting with a letter (it prefixes the tool names)"));
            }
            if RESERVED.contains(&name.as_str()) || presets::find(&name).is_some() {
                return Err(ApiError::bad(format!("`{name}` is reserved; choose another name")));
            }
            let url = check_url(body.url.as_deref().unwrap_or_default())?;
            let auth = custom_auth(body.auth.as_ref().unwrap_or(&AuthBody { kind: "bearer".into(), header: None }))?;
            let title = body.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(&name).to_string();
            (name, None, title, url, auth, json!({}))
        }
    };
    let headers = if preset.is_some() { BTreeMap::new() } else { clean_headers(&body.headers.clone().unwrap_or_default(), &auth)? };
    if connections::by_name(&app.db, &name).await?.is_some() {
        return Err(ApiError::Conflict(format!("`{name}` is already connected")));
    }
    save_secret(&app, &name, &auth, body.secret.as_deref(), true).await?;
    sqlx::query(
        "INSERT INTO connections (name, preset, title, url, auth, headers, options, created_by, updated_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)",
    )
    .bind(&name)
    .bind(preset)
    .bind(&title)
    .bind(&url)
    .bind(SqlJson(&auth))
    .bind(SqlJson(&headers))
    .bind(&options)
    .bind(actor.0.id)
    .execute(&app.db)
    .await?;
    app.activity(Some(actor.0.id), None, None, "integration.connected", &format!("{} connected {title}", actor.0.name), json!({ "connection": name })).await;
    saved(&app, &name).await
}

pub async fn update(State(app): State<App>, actor: AuthUser, Path(name): Path<String>, Json(body): Json<ConnectionBody>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let current = connections::by_name(&app.db, &name).await?.ok_or(ApiError::NotFound("connection"))?;
    let (title, url, auth, headers, options) = match current.preset() {
        Some(preset) => {
            let options = match &body.options {
                Some(options) => clean_options(preset, options),
                None => current.options.0.clone(),
            };
            (current.title.clone(), app.preset_url(preset, &options), current.auth.0.clone(), BTreeMap::new(), options)
        }
        None => {
            let auth = match &body.auth {
                Some(auth) => custom_auth(auth)?,
                None => current.auth.0.clone(),
            };
            let url = match &body.url {
                Some(url) => check_url(url)?,
                None => current.url.clone(),
            };
            let title = body.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).unwrap_or(current.title.clone());
            let headers = clean_headers(body.headers.as_ref().unwrap_or(&current.headers.0), &auth)?;
            (title, url, auth, headers, json!({}))
        }
    };
    // A credential for one server is not to be sent to another.
    let moved = url::Url::parse(&url).ok().map(|u| u.origin()) != url::Url::parse(&current.url).ok().map(|u| u.origin());
    let auth_changed = auth.header != current.auth.header;
    save_secret(&app, &name, &auth, body.secret.as_deref(), (moved || auth_changed) && auth.header.is_some()).await?;
    sqlx::query(
        "UPDATE connections SET title = $2, url = $3, auth = $4, headers = $5, options = $6, updated_by = $7, updated_at = now() WHERE name = $1",
    )
    .bind(&name)
    .bind(&title)
    .bind(&url)
    .bind(SqlJson(&auth))
    .bind(SqlJson(&headers))
    .bind(&options)
    .bind(actor.0.id)
    .execute(&app.db)
    .await?;
    saved(&app, &name).await
}

pub async fn delete(State(app): State<App>, actor: AuthUser, Path(name): Path<String>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let conn = connections::by_name(&app.db, &name).await?.ok_or(ApiError::NotFound("connection"))?;
    sqlx::query("DELETE FROM connections WHERE name = $1").bind(&name).execute(&app.db).await?;
    store::delete_secret(&app.db, &connections::secret_key(&name)).await?;
    app.mcp.reset();
    // A later connection of the same name must not inherit this one's grants.
    for table in ["users", "invites"] {
        let rows: Vec<(uuid::Uuid, SqlJson<GrantSet>)> = sqlx::query_as(&format!("SELECT id, grants FROM {table}")).fetch_all(&app.db).await?;
        for (id, SqlJson(mut grants)) in rows {
            let before = grants.clone();
            grants.remove_system(&name);
            if grants != before {
                sqlx::query(&format!("UPDATE {table} SET grants = $2 WHERE id = $1")).bind(id).bind(json!(grants)).execute(&app.db).await?;
            }
        }
    }
    app.activity(Some(actor.0.id), None, None, "integration.disconnected", &format!("{} disconnected {}", actor.0.name, conn.title), json!({ "connection": name })).await;
    Ok(Json(json!({ "ok": true })))
}

/// Ask the server again what it offers.
pub async fn refresh(State(app): State<App>, actor: AuthUser, Path(name): Path<String>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    connections::by_name(&app.db, &name).await?.ok_or(ApiError::NotFound("connection"))?;
    saved(&app, &name).await
}

#[derive(Deserialize)]
pub struct ApprovalsBody {
    /// Tool name → `read`, `write`, `hidden`, or `null` to undo a decision.
    approvals: BTreeMap<String, Option<ToolLevel>>,
}

/// Decide what tools are for. Each decision is tied to the tool as it is
/// described now; if a custom server changes the tool, it is hidden again.
pub async fn set_approvals(State(app): State<App>, actor: AuthUser, Path(name): Path<String>, Json(body): Json<ApprovalsBody>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let conn = connections::by_name(&app.db, &name).await?.ok_or(ApiError::NotFound("connection"))?;
    let mut approvals = conn.approvals.0.clone();
    for (tool, level) in &body.approvals {
        let known = conn.tools.iter().find(|t| &t.name == tool).ok_or_else(|| ApiError::bad(format!("{} has no tool named `{tool}`", conn.title)))?;
        match level {
            Some(level) => approvals.insert(tool.clone(), Approval { level: *level, fingerprint: known.fingerprint.clone() }),
            None => approvals.remove(tool),
        };
    }
    let conn: Connection = sqlx::query_as("UPDATE connections SET approvals = $2, updated_by = $3, updated_at = now() WHERE name = $1 RETURNING *")
        .bind(&name)
        .bind(SqlJson(&approvals))
        .bind(actor.0.id)
        .fetch_one(&app.db)
        .await?;
    Ok(Json(json!({ "connection": conn.view() })))
}

/// Reject grants on systems this installation does not have.
pub async fn check_grants(app: &App, grants: &GrantSet) -> ApiResult<()> {
    let catalog = app.grant_catalog().await?;
    for grant in grants.iter() {
        let id = grant.to_string();
        if !catalog.iter().any(|g| g["id"] == id.as_str()) {
            return Err(ApiError::bad(format!("`{id}` is not a permission on this installation")));
        }
    }
    Ok(())
}
