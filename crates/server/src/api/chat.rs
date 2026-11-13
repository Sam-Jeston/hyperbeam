//! Settings for the chat apps, and linking a chat account to a person.

use crate::app::App;
use crate::auth::AuthUser;
use crate::chat::{self, Kind};
use crate::error::{ApiError, ApiResult};
use crate::store;
use axum::Json;
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use hyperbeam_core::workflow::SessionRole;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;

fn kind(platform: &str) -> ApiResult<Kind> {
    Kind::parse(platform).ok_or(ApiError::NotFound("chat app"))
}

pub async fn list(State(app): State<App>, _user: AuthUser) -> ApiResult<Json<Value>> {
    let rows: Vec<(String, Value, DateTime<Utc>)> = sqlx::query_as("SELECT platform, config, updated_at FROM chat_apps").fetch_all(&app.db).await?;
    Ok(Json(json!({ "apps": chat::kinds_json(&app, &rows) })))
}

#[derive(Deserialize)]
pub struct AppBody {
    /// Tokens by name (`bot_token`, and `app_token` for Slack). Those left
    /// out keep their stored value.
    #[serde(default)]
    tokens: HashMap<String, String>,
    /// Slack: link people whose workspace address is a team member's.
    auto_link_email: Option<bool>,
}

pub async fn put(State(app): State<App>, actor: AuthUser, Path(platform): Path<String>, Json(body): Json<AppBody>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let kind = kind(&platform)?;
    let existing: Option<(Value,)> = sqlx::query_as("SELECT config FROM chat_apps WHERE platform = $1").bind(kind.as_str()).fetch_optional(&app.db).await?;

    // Check the tokens as they would be after this change, before storing any.
    let mut tokens: HashMap<&str, String> = HashMap::new();
    for (name, what) in kind.tokens() {
        let given = body.tokens.get(*name).map(|t| t.trim()).filter(|t| !t.is_empty()).map(str::to_string);
        let token = match given {
            Some(token) => token,
            None => store::get_secret(&app.db, &app.secrets, &kind.secret_key(name))
                .await?
                .ok_or_else(|| ApiError::bad(format!("{} needs its {what}", kind.title())))?,
        };
        tokens.insert(*name, token);
    }
    let info = chat::build(&app, kind, &tokens).check().await.map_err(|err| ApiError::bad(format!("{err:#}")))?;
    for (name, token) in &tokens {
        store::put_secret(&app.db, &app.secrets, &kind.secret_key(name), token).await?;
    }
    let previous = existing.as_ref().and_then(|(config,)| config["auto_link_email"].as_bool());
    let config = json!({
        "auto_link_email": body.auto_link_email.or(previous).unwrap_or(true),
        "workspace": info.workspace,
        "bot_name": info.bot_name,
    });
    sqlx::query(
        "INSERT INTO chat_apps (platform, config, updated_by) VALUES ($1, $2, $3)
         ON CONFLICT (platform) DO UPDATE SET config = $2, updated_by = $3, updated_at = now()",
    )
    .bind(kind.as_str())
    .bind(&config)
    .bind(actor.0.id)
    .execute(&app.db)
    .await?;
    chat::restart(&app, kind).await?;
    if existing.is_none() {
        app.activity(Some(actor.0.id), None, None, "chat.connected", &format!("{} connected {}", actor.0.name, kind.title()), json!({})).await;
    }
    Ok(Json(json!({ "ok": true, "config": config })))
}

pub async fn delete(State(app): State<App>, actor: AuthUser, Path(platform): Path<String>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let kind = kind(&platform)?;
    chat::remove(&app, kind).await?;
    app.activity(Some(actor.0.id), None, None, "chat.disconnected", &format!("{} disconnected {}", actor.0.name, kind.title()), json!({})).await;
    Ok(Json(json!({ "ok": true })))
}

/// The channels the bot is in, with which are enabled and how.
pub async fn channels(State(app): State<App>, actor: AuthUser, Path(platform): Path<String>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let kind = kind(&platform)?;
    let client = app.chat.platform(kind).ok_or_else(|| ApiError::bad(format!("{:#}", chat::not_connected(kind))))?;
    let available = client.channels().await.map_err(|err| ApiError::bad(format!("{err:#}")))?;
    let enabled: Vec<chat::Channel> = sqlx::query_as("SELECT channel_id, name, repos, harness, role FROM chat_channels WHERE platform = $1 ORDER BY name")
        .bind(kind.as_str())
        .fetch_all(&app.db)
        .await?;
    let mut list: Vec<Value> = available
        .iter()
        .map(|channel| {
            let settings = enabled.iter().find(|e| e.channel_id == channel.id);
            json!({
                "id": channel.id, "name": channel.name, "private": channel.private, "external": channel.external,
                "enabled": settings.is_some(),
                "repos": settings.map(|s| s.repos.0.clone()).unwrap_or_default(),
                "harness": settings.and_then(|s| s.harness.clone()),
                "role": settings.map(|s| s.role.clone()).unwrap_or_else(|| "worker".into()),
            })
        })
        .collect();
    // Enabled once, but the bot has since left or the channel is gone.
    for settings in enabled.iter().filter(|e| !available.iter().any(|c| c.id == e.channel_id)) {
        list.push(json!({
            "id": settings.channel_id, "name": settings.name, "private": false, "external": false, "enabled": true, "missing": true,
            "repos": settings.repos.0, "harness": settings.harness, "role": settings.role,
        }));
    }
    Ok(Json(json!({ "channels": list })))
}

#[derive(Deserialize)]
pub struct ChannelBody {
    enabled: bool,
    #[serde(default)]
    name: String,
    #[serde(default)]
    repos: Vec<String>,
    harness: Option<String>,
    #[serde(default)]
    role: SessionRole,
}

pub async fn put_channel(
    State(app): State<App>,
    actor: AuthUser,
    Path((platform, channel_id)): Path<(String, String)>,
    Json(body): Json<ChannelBody>,
) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let kind = kind(&platform)?;
    if !body.enabled {
        sqlx::query("DELETE FROM chat_channels WHERE platform = $1 AND channel_id = $2").bind(kind.as_str()).bind(&channel_id).execute(&app.db).await?;
        return Ok(Json(json!({ "ok": true })));
    }
    // What agents say in a thread is read by everyone in the channel.
    let client = app.chat.platform(kind).ok_or_else(|| ApiError::bad(format!("{:#}", chat::not_connected(kind))))?;
    let available = client.channels().await.map_err(|err| ApiError::bad(format!("{err:#}")))?;
    let channel = available.iter().find(|c| c.id == channel_id).ok_or_else(|| ApiError::bad("the bot is not in that channel. Add it to the channel first."))?;
    if channel.external {
        return Err(ApiError::bad("that channel is shared with another organisation, so agents cannot be started in it"));
    }
    let mut repos = Vec::new();
    for name in &body.repos {
        let repo = store::repo_by_name(&app.db, name).await?.ok_or_else(|| ApiError::bad(format!("repository `{name}` is not registered")))?;
        if !repos.contains(&repo.full_name) {
            repos.push(repo.full_name);
        }
    }
    let harness = body.harness.as_deref().map(str::trim).filter(|h| !h.is_empty());
    if let Some(name) = harness {
        store::harness_by_name(&app.db, name).await?.ok_or_else(|| ApiError::bad(format!("no harness profile named `{name}`")))?;
    }
    sqlx::query(
        "INSERT INTO chat_channels (platform, channel_id, name, repos, harness, role) VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (platform, channel_id) DO UPDATE SET name = $3, repos = $4, harness = $5, role = $6, updated_at = now()",
    )
    .bind(kind.as_str())
    .bind(&channel_id)
    .bind(if body.name.trim().is_empty() { &channel.name } else { &body.name })
    .bind(json!(repos))
    .bind(harness)
    .bind(body.role.as_str())
    .execute(&app.db)
    .await?;
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------------ linking

/// Whose chat account a one-time link would connect, so the person can see
/// what they are agreeing to.
pub async fn link_info(State(app): State<App>, _user: AuthUser, Path(code): Path<String>) -> ApiResult<Json<Value>> {
    let (kind, _, name) = chat::link_code(&app, &code).await?;
    Ok(Json(json!({ "platform": kind.as_str(), "display_name": name })))
}

pub async fn link(State(app): State<App>, AuthUser(user): AuthUser, Path(code): Path<String>) -> ApiResult<Json<Value>> {
    let kind = chat::redeem_link(&app, &code, &user).await?;
    Ok(Json(json!({ "ok": true, "platform": kind.as_str() })))
}

pub async fn my_identities(State(app): State<App>, AuthUser(user): AuthUser) -> ApiResult<Json<Value>> {
    let rows: Vec<(String, String, String, String, DateTime<Utc>)> =
        sqlx::query_as("SELECT platform, external_id, display_name, linked_by, linked_at FROM chat_identities WHERE user_id = $1 ORDER BY linked_at")
            .bind(user.id)
            .fetch_all(&app.db)
            .await?;
    let list: Vec<Value> = rows
        .into_iter()
        .map(|(platform, external_id, display_name, linked_by, linked_at)| {
            json!({ "platform": platform, "external_id": external_id, "display_name": display_name, "linked_by": linked_by, "linked_at": linked_at })
        })
        .collect();
    Ok(Json(json!({ "identities": list })))
}

/// Unlink a chat account. With linking by email on, an account whose
/// workspace address is still yours is linked again the next time it is used.
pub async fn unlink(State(app): State<App>, AuthUser(user): AuthUser, Path((platform, external_id)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    let kind = kind(&platform)?;
    let removed = sqlx::query("DELETE FROM chat_identities WHERE platform = $1 AND external_id = $2 AND user_id = $3")
        .bind(kind.as_str())
        .bind(&external_id)
        .bind(user.id)
        .execute(&app.db)
        .await?
        .rows_affected();
    if removed == 0 {
        return Err(ApiError::NotFound("linked account"));
    }
    Ok(Json(json!({ "ok": true })))
}
