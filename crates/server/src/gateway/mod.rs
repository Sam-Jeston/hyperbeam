//! The tool gateway: an MCP server (streamable HTTP, JSON responses) that is
//! the only way an agent can act on GitHub, Linear, Notion or the platform.
//!
//! Every call is authenticated by the session's run token, checked against
//! the session's grants, audited, and shown in the session log.

mod github;
mod linear;
mod notion;
mod platform;

use crate::app::App;
use crate::auth::bearer;
use crate::store::{self, Session};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use noemune_core::Grant;
use noemune_core::protocol::event;
use serde_json::{Value, json};

pub enum ToolError {
    /// The session is not permitted to do this.
    Denied(String),
    /// Permitted, but it did not work.
    Failed(String),
}

impl From<anyhow::Error> for ToolError {
    fn from(err: anyhow::Error) -> Self {
        ToolError::Failed(format!("{err:#}"))
    }
}

impl From<sqlx::Error> for ToolError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!("gateway database error: {err}");
        ToolError::Failed("the platform hit a database error".into())
    }
}

pub type ToolResult = Result<ToolOutput, ToolError>;

pub struct ToolOutput {
    /// Returned to the agent.
    pub text: String,
    /// What was acted on, for the audit trail (e.g. `acme/api#12`).
    pub target: String,
}

impl ToolOutput {
    pub fn new(target: impl Into<String>, text: impl Into<String>) -> Self {
        Self { text: text.into(), target: target.into() }
    }
}

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
    /// Grant the session must hold. `None` for platform tools.
    pub grant: Option<Grant>,
    pub conductor_only: bool,
}

/// Build a JSON schema for a tool's arguments: `(name, type, description, required)`.
pub fn schema(props: &[(&str, &str, &str, bool)]) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, ty, description, is_required) in props {
        let prop = match *ty {
            "string[]" => json!({ "type": "array", "items": { "type": "string" }, "description": description }),
            other => json!({ "type": other, "description": description }),
        };
        properties.insert(name.to_string(), prop);
        if *is_required {
            required.push(*name);
        }
    }
    json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
}

pub fn arg_str<'a>(args: &'a Value, name: &str) -> Result<&'a str, ToolError> {
    args.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ToolError::Failed(format!("missing required argument `{name}`")))
}

pub fn arg_opt<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

pub fn arg_u64(args: &Value, name: &str) -> Result<u64, ToolError> {
    let value = args.get(name);
    value
        .and_then(Value::as_u64)
        .or_else(|| value.and_then(Value::as_str).and_then(|s| s.trim_start_matches('#').parse().ok()))
        .ok_or_else(|| ToolError::Failed(format!("missing required numeric argument `{name}`")))
}

pub fn arg_list(args: &Value, name: &str) -> Vec<String> {
    args.get(name)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn catalog() -> Vec<ToolDef> {
    let mut tools = platform::tools();
    tools.extend(github::tools());
    tools.extend(linear::tools());
    tools.extend(notion::tools());
    tools
}

/// Tools this session can see: it holds the grant, and the system is connected.
async fn visible_tools(app: &App, session: &Session) -> Vec<ToolDef> {
    let connected: Vec<String> = sqlx::query_as::<_, (String,)>("SELECT kind FROM integrations")
        .fetch_all(&app.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(kind,)| kind)
        .collect();
    catalog()
        .into_iter()
        .filter(|tool| match tool.grant {
            Some(grant) => session.grants.has(grant) && connected.iter().any(|kind| kind == grant.system()),
            None => true,
        })
        .filter(|tool| !tool.conductor_only || session.role == "conductor")
        .collect()
}

async fn call_tool(app: &App, session: &Session, name: &str, args: &Value) -> ToolResult {
    let Some(tool) = catalog().into_iter().find(|t| t.name == name) else {
        return Err(ToolError::Failed(format!("no tool named `{name}`")));
    };
    if let Some(grant) = tool.grant {
        if !session.grants.has(grant) {
            return Err(ToolError::Denied(format!(
                "this session does not hold `{grant}` ({}). Its permissions come from the person who started it.",
                grant.label()
            )));
        }
    }
    if tool.conductor_only && session.role != "conductor" {
        return Err(ToolError::Denied("only conductor sessions can manage other agents".into()));
    }
    match name.split('_').next() {
        Some("github") => github::call(app, session, name, args).await,
        Some("linear") => linear::call(app, session, name, args).await,
        Some("notion") => notion::call(app, session, name, args).await,
        _ => platform::call(app, session, name, args).await,
    }
}

/// A repository argument must be one of the session's own repositories.
pub fn session_repo(session: &Session, args: &Value) -> Result<String, ToolError> {
    let wanted = match arg_opt(args, "repo") {
        Some(repo) => repo.to_string(),
        None if session.repos.len() == 1 => session.repos[0].clone(),
        None => return Err(ToolError::Failed("missing required argument `repo` (owner/name)".into())),
    };
    session
        .repos
        .iter()
        .find(|r| r.eq_ignore_ascii_case(&wanted))
        .cloned()
        .ok_or_else(|| {
            let have = if session.repos.is_empty() { "it has none".to_string() } else { session.repos.join(", ") };
            ToolError::Denied(format!("`{wanted}` is not one of this session's repositories ({have})"))
        })
}

fn rpc_result(id: Value, result: Value) -> Response {
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

fn rpc_error(id: Value, code: i64, message: &str) -> Response {
    Json(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })).into_response()
}

pub async fn handle(State(app): State<App>, parts: Parts, Json(request): Json<Value>) -> Response {
    let Some(token) = bearer(&parts) else { return StatusCode::UNAUTHORIZED.into_response() };
    let session = match app.session_for_token(&token).await {
        Ok(Some(session)) => session,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let method = request.get("method").and_then(Value::as_str).unwrap_or_default();
    let Some(id) = request.get("id").cloned().filter(|id| !id.is_null()) else {
        // Notifications (e.g. notifications/initialized) need no reply.
        return StatusCode::ACCEPTED.into_response();
    };
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18");
            rpc_result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "noemune", "title": "Noemune", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "Tools for acting on GitHub, Linear, Notion and the Noemune platform. \
                                     Calls are checked against this session's permissions and are visible to the team.",
                }),
            )
        }
        "ping" => rpc_result(id, json!({})),
        "tools/list" => {
            let tools: Vec<Value> = visible_tools(&app, &session)
                .await
                .into_iter()
                .map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": t.schema }))
                .collect();
            rpc_result(id, json!({ "tools": tools }))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let outcome = call_tool(&app, &session, &name, &args).await;
            // What a refused or failed call was aimed at, for the audit trail.
            let aimed_at = ["repo", "id", "page_id", "key", "title"]
                .iter()
                .find_map(|k| args.get(*k).and_then(Value::as_str))
                .map(|v| v.chars().take(120).collect::<String>())
                .unwrap_or_default();
            let (text, target, allowed, failed) = match outcome {
                Ok(out) => (out.text, out.target, true, false),
                Err(ToolError::Denied(why)) => (format!("Denied: {why}"), aimed_at, false, true),
                Err(ToolError::Failed(why)) => (format!("Error: {why}"), aimed_at, true, true),
            };
            // Reads of the platform's own state are not worth a log line each.
            let quiet = matches!(name.as_str(), "noemune_memory_list" | "noemune_memory_read" | "noemune_list_agents" | "noemune_get_agent") && !failed;
            let detail = if failed { text.chars().take(500).collect::<String>() } else { String::new() };
            store::record_audit(&app.db, &session, &name, &target, allowed, &detail).await.ok();
            if !quiet {
                let summary = match (allowed, failed) {
                    (false, _) if target.is_empty() => format!("{name} denied"),
                    (false, _) => format!("{name} denied: {target}"),
                    (true, true) => format!("{name} failed"),
                    (true, false) if target.is_empty() => name.clone(),
                    (true, false) => format!("{name}: {target}"),
                };
                app.append_event(
                    session.id,
                    event::GATEWAY,
                    json!({ "tool": name, "target": target, "allowed": allowed, "failed": failed, "summary": summary, "detail": detail }),
                )
                .await
                .ok();
            }
            rpc_result(id, json!({ "content": [{ "type": "text", "text": text }], "isError": failed }))
        }
        _ => rpc_error(id, -32601, "method not found"),
    }
}

/// Streamable HTTP clients may probe for a server-initiated stream; there is none.
pub async fn no_stream() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}
