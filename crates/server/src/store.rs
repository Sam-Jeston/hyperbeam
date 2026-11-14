//! Rows and the queries shared across modules.

use crate::crypto::SecretBox;
use anyhow::Result;
use chrono::{DateTime, Utc};
use hyperbeam_core::{GrantSet, RepoScope, Role};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use sqlx::types::Json;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    #[serde(skip)]
    pub password_hash: String,
    pub role: String,
    pub grants: Json<GrantSet>,
    pub repo_scope: Option<Json<Vec<String>>>,
    pub disabled: bool,
    pub created_at: DateTime<Utc>,
}

impl User {
    pub fn role(&self) -> Role {
        self.role.parse().unwrap_or(Role::Member)
    }

    pub fn is_admin(&self) -> bool {
        self.role().is_admin()
    }

    pub fn repo_scope(&self) -> RepoScope {
        match &self.repo_scope {
            None => RepoScope::all(),
            Some(Json(list)) => RepoScope::only(list.iter().cloned()),
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Repo {
    pub id: Uuid,
    /// The code host it is on: `github` or `gitlab`.
    pub host: String,
    pub full_name: String,
    pub default_branch: String,
    pub description: String,
    /// The environment agents working on it run in unless told otherwise.
    pub environment_id: Option<Uuid>,
    /// Run with `sh -c` in each fresh checkout, before the agent starts.
    pub setup_command: String,
    pub created_at: DateTime<Utc>,
}

/// A container image holding a team's toolchains. The coding agent is carried
/// into it from the harness image when a sandbox starts.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Environment {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub image: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CredentialSlot {
    /// Environment variable the harness reads.
    pub env: String,
    pub label: String,
    /// `api_key` may be shared by the team; `oauth` is a personal
    /// subscription token and may only be set by its owner for themselves.
    pub kind: String,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Harness {
    pub id: Uuid,
    pub name: String,
    pub title: String,
    pub description: String,
    pub image: String,
    pub command: Json<Vec<String>>,
    pub env: Json<std::collections::BTreeMap<String, String>>,
    pub credentials: Json<Vec<CredentialSlot>>,
    pub egress_hosts: Json<Vec<String>>,
    pub builtin: bool,
    /// Models people may pick. Empty means whatever the harness offers.
    pub models: Json<Vec<ModelChoice>>,
    /// What the harness itself last reported.
    pub discovered_models: Json<Vec<ModelChoice>>,
    pub default_model: Option<String>,
    pub model_env: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelChoice {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub description: String,
}

impl Harness {
    /// The models offered when starting work on this harness.
    pub fn choices(&self) -> &[ModelChoice] {
        if self.models.is_empty() { &self.discovered_models } else { &self.models }
    }
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Skill {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub content: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Workflow {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub yaml: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Link {
    pub kind: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Workstream {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub status: String,
    pub repos: Json<Vec<String>>,
    pub links: Json<Vec<Link>>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A question an agent asked with `hyperbeam_ask_user` that nobody has
/// answered yet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Question {
    pub id: Uuid,
    pub text: String,
    /// Answers the agent suggested. People may always answer in their own words.
    #[serde(default)]
    pub options: Vec<String>,
    /// Who is expected to answer: `team`, or `conductor` for a worker whose
    /// conductor is told first.
    pub to: String,
    pub asked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Session {
    pub id: Uuid,
    pub workstream_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub title: String,
    pub role: String,
    pub status: String,
    pub harness_id: Uuid,
    /// The environment its sandboxes run in; `None` is the harness image.
    pub environment_id: Option<Uuid>,
    pub created_by: Uuid,
    pub workflow: Option<String>,
    pub repos: Json<Vec<String>>,
    pub skills: Json<Vec<String>>,
    pub grants: Json<GrantSet>,
    pub work_branch: String,
    /// Requested model; `None` means the harness profile's default.
    pub model: Option<String>,
    pub links: Json<Vec<Link>>,
    pub headline: String,
    pub summary: String,
    pub error: Option<String>,
    /// The open question, if the agent is waiting on an answer.
    pub question: Option<Json<Question>>,
    pub event_seq: i64,
    pub launches: i32,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub last_activity_at: DateTime<Utc>,
}

pub const SESSION_COLUMNS: &str = "id, workstream_id, parent_id, title, role, status, harness_id, environment_id, created_by, \
     workflow, repos, skills, grants, work_branch, model, links, headline, summary, error, question, event_seq, launches, \
     created_at, started_at, ended_at, last_activity_at";

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Event {
    pub session_id: Uuid,
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub kind: String,
    pub data: Json<Value>,
}

pub async fn user_by_id(db: &PgPool, id: Uuid) -> sqlx::Result<Option<User>> {
    sqlx::query_as("SELECT * FROM users WHERE id = $1").bind(id).fetch_optional(db).await
}

pub async fn session_by_id(db: &PgPool, id: Uuid) -> sqlx::Result<Option<Session>> {
    sqlx::query_as(&format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE id = $1"))
        .bind(id)
        .fetch_optional(db)
        .await
}

pub async fn session_by_token(db: &PgPool, token: &str) -> sqlx::Result<Option<Session>> {
    sqlx::query_as(&format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE run_token_hash = $1"))
        .bind(crate::crypto::hash_token(token))
        .fetch_optional(db)
        .await
}

pub async fn harness_by_id(db: &PgPool, id: Uuid) -> sqlx::Result<Option<Harness>> {
    sqlx::query_as("SELECT * FROM harnesses WHERE id = $1").bind(id).fetch_optional(db).await
}

pub async fn harness_by_name(db: &PgPool, name: &str) -> sqlx::Result<Option<Harness>> {
    sqlx::query_as("SELECT * FROM harnesses WHERE name = $1").bind(name).fetch_optional(db).await
}

pub async fn environment_by_id(db: &PgPool, id: Uuid) -> sqlx::Result<Option<Environment>> {
    sqlx::query_as("SELECT * FROM environments WHERE id = $1").bind(id).fetch_optional(db).await
}

pub async fn environment_by_name(db: &PgPool, name: &str) -> sqlx::Result<Option<Environment>> {
    sqlx::query_as("SELECT * FROM environments WHERE name = $1").bind(name).fetch_optional(db).await
}

pub async fn repo_by_name(db: &PgPool, full_name: &str) -> sqlx::Result<Option<Repo>> {
    sqlx::query_as("SELECT * FROM repos WHERE lower(full_name) = lower($1)")
        .bind(full_name)
        .fetch_optional(db)
        .await
}

pub async fn get_setting(db: &PgPool, key: &str) -> sqlx::Result<Option<Value>> {
    let row: Option<(Value,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = $1").bind(key).fetch_optional(db).await?;
    Ok(row.map(|r| r.0))
}

pub async fn set_setting(db: &PgPool, key: &str, value: &Value) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ($1, $2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = now()",
    )
    .bind(key)
    .bind(value)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn put_secret(db: &PgPool, sb: &SecretBox, key: &str, value: &str) -> Result<()> {
    let (nonce, ciphertext) = sb.seal(key, value)?;
    sqlx::query(
        "INSERT INTO secrets (key, nonce, ciphertext) VALUES ($1, $2, $3)
         ON CONFLICT (key) DO UPDATE SET nonce = excluded.nonce, ciphertext = excluded.ciphertext, updated_at = now()",
    )
    .bind(key)
    .bind(nonce)
    .bind(ciphertext)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_secret(db: &PgPool, sb: &SecretBox, key: &str) -> Result<Option<String>> {
    let row: Option<(Vec<u8>, Vec<u8>)> =
        sqlx::query_as("SELECT nonce, ciphertext FROM secrets WHERE key = $1").bind(key).fetch_optional(db).await?;
    match row {
        Some((nonce, ciphertext)) => Ok(Some(sb.open(key, &nonce, &ciphertext)?)),
        None => Ok(None),
    }
}

pub async fn delete_secret(db: &PgPool, key: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM secrets WHERE key = $1").bind(key).execute(db).await?;
    Ok(())
}

pub async fn record_activity(
    db: &PgPool,
    user_id: Option<Uuid>,
    session_id: Option<Uuid>,
    workstream_id: Option<Uuid>,
    kind: &str,
    summary: &str,
    data: Value,
) -> sqlx::Result<Value> {
    let (id, ts): (i64, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO activity (user_id, session_id, workstream_id, kind, summary, data)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, ts",
    )
    .bind(user_id)
    .bind(session_id)
    .bind(workstream_id)
    .bind(kind)
    .bind(summary)
    .bind(&data)
    .fetch_one(db)
    .await?;
    Ok(serde_json::json!({
        "id": id, "ts": ts, "user_id": user_id, "session_id": session_id,
        "workstream_id": workstream_id, "kind": kind, "summary": summary, "data": data,
    }))
}

pub async fn record_audit(
    db: &PgPool,
    session: &Session,
    action: &str,
    target: &str,
    allowed: bool,
    detail: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO audit (session_id, user_id, action, target, allowed, detail) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(session.id)
    .bind(session.created_by)
    .bind(action)
    .bind(target)
    .bind(allowed)
    .bind(detail)
    .execute(db)
    .await?;
    Ok(())
}

/// Harness profiles shipped with the platform. `{runner}` in a command is
/// replaced with the runner binary's own path inside the sandbox.
pub async fn seed_harnesses(db: &PgPool) -> sqlx::Result<()> {
    let slot = |env: &str, label: &str, kind: &str| serde_json::json!({ "env": env, "label": label, "kind": kind });
    let builtins: [(&str, &str, &str, &str, Value, Value, Value, Option<&str>); 4] = [
        (
            "claude-code",
            "Claude Code",
            "Anthropic's coding agent, via the Agent Client Protocol adapter.",
            "ghcr.io/sam-jeston/hyperbeam/harness-claude-code:latest",
            serde_json::json!(["claude-agent-acp"]),
            serde_json::json!([
                slot("ANTHROPIC_API_KEY", "Anthropic API key", "api_key"),
                slot("CLAUDE_CODE_OAUTH_TOKEN", "Claude subscription token (claude setup-token)", "oauth"),
            ]),
            serde_json::json!(["api.anthropic.com", "*.anthropic.com", "claude.ai", "*.claude.com"]),
            Some("ANTHROPIC_MODEL"),
        ),
        (
            "codex",
            "Codex",
            "OpenAI's coding agent, via the Agent Client Protocol adapter.",
            "ghcr.io/sam-jeston/hyperbeam/harness-codex:latest",
            serde_json::json!(["codex-acp"]),
            serde_json::json!([slot("OPENAI_API_KEY", "OpenAI API key", "api_key")]),
            serde_json::json!(["api.openai.com", "*.openai.com", "chatgpt.com"]),
            None,
        ),
        (
            "gemini",
            "Gemini CLI",
            "Google's coding agent; speaks the Agent Client Protocol natively.",
            "ghcr.io/sam-jeston/hyperbeam/harness-gemini:latest",
            serde_json::json!(["gemini", "--acp"]),
            serde_json::json!([slot("GEMINI_API_KEY", "Gemini API key", "api_key")]),
            serde_json::json!(["generativelanguage.googleapis.com", "*.googleapis.com"]),
            Some("GEMINI_MODEL"),
        ),
        (
            "opencode",
            "OpenCode",
            "Open-source coding agent; speaks the Agent Client Protocol natively.",
            "ghcr.io/sam-jeston/hyperbeam/harness-opencode:latest",
            serde_json::json!(["opencode", "acp"]),
            serde_json::json!([
                slot("ANTHROPIC_API_KEY", "Anthropic API key", "api_key"),
                slot("OPENAI_API_KEY", "OpenAI API key", "api_key"),
                slot("OPENROUTER_API_KEY", "OpenRouter API key", "api_key"),
            ]),
            serde_json::json!(["api.anthropic.com", "api.openai.com", "openrouter.ai", "models.dev", "opencode.ai"]),
            None,
        ),
    ];
    for (name, title, description, image, command, credentials, egress, model_env) in builtins {
        // Insert once; never overwrite an admin's edits to a builtin profile.
        sqlx::query(
            "INSERT INTO harnesses (id, name, title, description, image, command, credentials, egress_hosts, model_env, builtin)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, true) ON CONFLICT (name) DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(name)
        .bind(title)
        .bind(description)
        .bind(image)
        .bind(command)
        .bind(credentials)
        .bind(egress)
        .bind(model_env)
        .execute(db)
        .await?;
    }
    Ok(())
}
