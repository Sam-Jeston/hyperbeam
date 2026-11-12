//! MCP connections: first-class (preset) and custom.

use crate::app::App;
use crate::auth::AuthUser;
use crate::connections::oauth::{self, Client, Spec};
use crate::connections::{self, Approval, Auth, Connection, RESERVED, ToolLevel, presets};
use crate::error::{ApiError, ApiResult};
use crate::store;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::Redirect;
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
    Ok(Json(json!({
        "presets": presets,
        "connections": conns.iter().map(Connection::view).collect::<Vec<_>>(),
        // What an admin registers when creating an OAuth client by hand.
        "oauth_redirect_uri": app.oauth_redirect_uri(),
    })))
}

/// Every permission that can be granted here, for the people screen.
pub async fn grants(State(app): State<App>, _user: AuthUser) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "grants": app.grant_catalog().await? })))
}

#[derive(Deserialize)]
pub struct AuthBody {
    /// `bearer`, `header`, `oauth` or `none`.
    kind: String,
    #[serde(default)]
    header: Option<String>,
    /// OAuth: a client the admin created with the vendor. Without one, the
    /// server is asked to register Hyperbeam itself.
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
    /// OAuth: scopes to ask for, space-separated. Defaults to what the server suggests.
    #[serde(default)]
    scope: Option<String>,
}

fn trimmed(value: &Option<String>) -> Option<String> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

impl AuthBody {
    fn is_oauth(&self) -> bool {
        self.kind == "oauth"
    }

    fn client(&self) -> ApiResult<Option<Client>> {
        match (trimmed(&self.client_id), trimmed(&self.client_secret)) {
            (Some(id), secret) => Ok(Some(Client { id, secret })),
            (None, Some(_)) => Err(ApiError::bad("a client secret needs its client ID")),
            (None, None) => Ok(None),
        }
    }
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
        // An OAuth token is sent as a bearer token; the sign-in fills in the rest.
        "bearer" | "oauth" => Ok(Auth { header: Some("Authorization".into()), prefix: "Bearer ".into(), oauth: None }),
        "header" => {
            let header = check_header(body.header.as_deref().unwrap_or_default())?;
            Ok(Auth { header: Some(header), prefix: String::new(), oauth: None })
        }
        "none" => Ok(Auth::default()),
        _ => Err(ApiError::bad("auth must be `bearer`, `header`, `oauth` or `none`")),
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
            let auth = Auth { header: Some(preset.auth_header.into()), prefix: preset.auth_prefix.into(), oauth: None };
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
            let auth = custom_auth(body.auth.as_ref().unwrap_or(&AuthBody { kind: "bearer".into(), header: None, client_id: None, client_secret: None, scope: None }))?;
            let title = body.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(&name).to_string();
            (name, None, title, url, auth, json!({}))
        }
    };
    let headers = if preset.is_some() { BTreeMap::new() } else { clean_headers(&body.headers.clone().unwrap_or_default(), &auth)? };
    if connections::by_name(&app.db, &name).await?.is_some() {
        return Err(ApiError::Conflict(format!("`{name}` is already connected")));
    }
    let custom_oauth = body.auth.as_ref().filter(|a| preset.is_none() && a.is_oauth());
    if custom_oauth.is_some() || preset.and_then(presets::find).is_some_and(|p| p.oauth) {
        // Nothing is saved until the admin comes back signed in.
        let client = custom_oauth.map(AuthBody::client).transpose()?.flatten();
        let scope = custom_oauth.and_then(|a| trimmed(&a.scope));
        let spec = Spec { name, preset: preset.map(str::to_string), title, url, headers, options };
        return sign_in(&app, spec, client, None, scope, &actor).await;
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

/// Start an OAuth sign-in and tell the browser where to go.
async fn sign_in(app: &App, spec: Spec, client: Option<Client>, existing: Option<&Connection>, scope: Option<String>, actor: &AuthUser) -> ApiResult<Json<Value>> {
    // What goes wrong here is about the server the admin named, and theirs to fix.
    let url = app.begin_oauth(spec, client, existing, scope, actor.0.id).await.map_err(|err| ApiError::bad(format!("{err:#}")))?;
    Ok(Json(json!({ "authorize_url": url })))
}

/// Sign in again to a connection that uses OAuth, e.g. after access was revoked.
pub async fn authorize(State(app): State<App>, actor: AuthUser, Path(name): Path<String>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let current = connections::by_name(&app.db, &name).await?.ok_or(ApiError::NotFound("connection"))?;
    let Some(oauth) = &current.auth.oauth else {
        return Err(ApiError::bad(format!("{} does not sign in with OAuth", current.title)));
    };
    let spec = Spec {
        name: current.name.clone(),
        preset: current.preset.clone(),
        title: current.title.clone(),
        url: current.url.clone(),
        headers: current.headers.0.clone(),
        options: current.options.0.clone(),
    };
    // Scopes an admin chose stand; otherwise ask the server again.
    let scope = oauth.scope.clone();
    sign_in(&app, spec, None, Some(&current), scope, &actor).await
}

#[derive(Deserialize)]
pub struct CallbackParams {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Where an authorization server sends the admin's browser back to. Always
/// answers by returning them to the integrations screen, with the outcome.
pub async fn oauth_callback(State(app): State<App>, actor: Result<AuthUser, ApiError>, Query(params): Query<CallbackParams>) -> Redirect {
    let (key, value) = match finish_sign_in(&app, actor, params).await {
        Ok(name) => ("connected", name),
        Err(why) => ("oauth_error", why),
    };
    let value: String = url::form_urlencoded::byte_serialize(value.as_bytes()).collect();
    Redirect::to(&format!("/settings/integrations?{key}={value}"))
}

async fn finish_sign_in(app: &App, actor: Result<AuthUser, ApiError>, params: CallbackParams) -> Result<String, String> {
    let pending = params.state.as_deref().and_then(|state| app.claim_oauth(state)).ok_or("That sign-in has expired or was already finished. Start it again.")?;
    let title = pending.spec.title.clone();
    // The session cookie comes along, so this is the admin who started it.
    let actor = actor.map_err(|_| "Sign in to Hyperbeam, then connect again.")?;
    if actor.0.id != pending.actor || !actor.0.is_admin() {
        return Err("That sign-in was started by someone else. Start it again.".into());
    }
    if let Some(error) = &params.error {
        let why = params.error_description.as_deref().filter(|d| !d.is_empty()).unwrap_or(error);
        return Err(format!("{title} did not grant access: {}", why.chars().take(300).collect::<String>()));
    }
    let code = params.code.as_deref().filter(|c| !c.is_empty()).ok_or_else(|| format!("{title} sent no authorization code. Start again."))?;
    let tokens = app.redeem_oauth(&pending, code).await.map_err(|err| format!("{err:#}"))?;
    let name = pending.spec.name.clone();
    let internal = |err: anyhow::Error| {
        tracing::error!("saving the sign-in for {name}: {err:#}");
        "Something went wrong on the server; see its logs.".to_string()
    };
    let existed = connections::by_name(&app.db, &name).await.map_err(|e| internal(e.into()))?.is_some();
    app.save_oauth(&pending, &tokens).await.map_err(internal)?;
    if !existed {
        app.activity(Some(actor.0.id), None, None, "integration.connected", &format!("{} connected {title}", actor.0.name), json!({ "connection": name })).await;
    }
    app.mcp.reset();
    app.refresh_tools(&name).await.map_err(internal)?;
    Ok(name)
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
    let signed_in = current.auth.oauth.as_ref();
    let asked = body.auth.as_ref().filter(|a| current.preset.is_none() && a.is_oauth());
    if asked.is_some() || signed_in.is_some() && (body.auth.is_none() || current.preset.is_some()) {
        let supplied = signed_in.filter(|o| !o.registered).map(|o| o.client_id.clone());
        let client = asked.map(AuthBody::client).transpose()?.flatten();
        let scope = asked.and_then(|a| trimmed(&a.scope));
        let again = match (signed_in, asked) {
            (None, _) => true,
            (Some(_), _) if moved => true,
            (Some(now), Some(_)) => {
                client.as_ref().map(|c| &c.id) != supplied.as_ref() || client.as_ref().is_some_and(|c| c.secret.is_some()) || scope.as_ref().is_some_and(|s| Some(s) != now.scope.as_ref())
            }
            (Some(_), None) => false,
        };
        if again {
            // The same client with no new secret keeps the secret it has.
            let client = client.filter(|c| c.secret.is_some() || Some(&c.id) != supplied.as_ref());
            let scope = scope.or_else(|| signed_in.filter(|_| !moved).and_then(|o| o.scope.clone()));
            let spec = Spec { name, preset: current.preset.clone(), title, url, headers, options };
            return sign_in(&app, spec, client, Some(&current), scope, &actor).await;
        }
        // Nothing about the sign-in changed: keep it, and its tokens.
        sqlx::query("UPDATE connections SET title = $2, url = $3, headers = $4, options = $5, updated_by = $6, updated_at = now() WHERE name = $1")
            .bind(&name)
            .bind(&title)
            .bind(&url)
            .bind(SqlJson(&headers))
            .bind(&options)
            .bind(actor.0.id)
            .execute(&app.db)
            .await?;
        return saved(&app, &name).await;
    }
    let auth_changed = auth.header != current.auth.header || signed_in.is_some();
    save_secret(&app, &name, &auth, body.secret.as_deref(), (moved || auth_changed) && auth.header.is_some()).await?;
    oauth::delete_secrets(&app.db, &name).await?;
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
    oauth::delete_secrets(&app.db, &name).await?;
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
