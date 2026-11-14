//! A person's notifications, and where else they are sent.

use crate::app::App;
use crate::auth::AuthUser;
use crate::chat::Kind;
use crate::error::{ApiError, ApiResult};
use crate::notify::{COLUMNS, Notification};
use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct ListParams {
    limit: Option<i64>,
}

pub async fn list(State(app): State<App>, AuthUser(user): AuthUser, Query(params): Query<ListParams>) -> ApiResult<Json<Value>> {
    let notifications: Vec<Notification> =
        sqlx::query_as(&format!("SELECT {COLUMNS} FROM notifications WHERE user_id = $1 ORDER BY created_at DESC, id DESC LIMIT $2"))
            .bind(user.id)
            .bind(params.limit.unwrap_or(100).clamp(1, 500))
            .fetch_all(&app.db)
            .await?;
    let (unread,): (i64,) = sqlx::query_as("SELECT count(*) FROM notifications WHERE user_id = $1 AND read_at IS NULL").bind(user.id).fetch_one(&app.db).await?;
    Ok(Json(json!({ "notifications": notifications, "unread": unread })))
}

#[derive(Deserialize)]
pub struct ReadBody {
    /// One notification.
    id: Option<Uuid>,
    /// Every notification about one session.
    session_id: Option<Uuid>,
}

/// Mark notifications as read. With neither field, all of them.
pub async fn read(State(app): State<App>, AuthUser(user): AuthUser, Json(body): Json<ReadBody>) -> ApiResult<Json<Value>> {
    let read = app.read_notifications(user.id, body.id, body.session_id).await?;
    Ok(Json(json!({ "ok": true, "read": read })))
}

/// The chat apps this person could be notified through, and which are on.
pub async fn channels(State(app): State<App>, AuthUser(user): AuthUser) -> ApiResult<Json<Value>> {
    let enabled: Vec<(String,)> = sqlx::query_as("SELECT platform FROM notification_channels WHERE user_id = $1").bind(user.id).fetch_all(&app.db).await?;
    let linked: Vec<(String, String)> =
        sqlx::query_as("SELECT platform, display_name FROM chat_identities WHERE user_id = $1 ORDER BY linked_at").bind(user.id).fetch_all(&app.db).await?;
    let connected: Vec<(String,)> = sqlx::query_as("SELECT platform FROM chat_apps").fetch_all(&app.db).await?;
    let list: Vec<Value> = Kind::ALL
        .into_iter()
        .map(|kind| {
            let accounts: Vec<&str> = linked.iter().filter(|(platform, _)| platform == kind.as_str()).map(|(_, name)| name.as_str()).collect();
            json!({
                "platform": kind.as_str(),
                "enabled": enabled.iter().any(|(platform,)| platform == kind.as_str()),
                // An admin has connected the bot.
                "connected": connected.iter().any(|(platform,)| platform == kind.as_str()),
                // The chat accounts a message would go to.
                "accounts": accounts,
            })
        })
        .collect();
    Ok(Json(json!({ "channels": list })))
}

#[derive(Deserialize)]
pub struct ChannelBody {
    enabled: bool,
}

pub async fn put_channel(State(app): State<App>, AuthUser(user): AuthUser, Path(platform): Path<String>, Json(body): Json<ChannelBody>) -> ApiResult<Json<Value>> {
    let kind = Kind::parse(&platform).ok_or(ApiError::NotFound("chat app"))?;
    if !body.enabled {
        sqlx::query("DELETE FROM notification_channels WHERE user_id = $1 AND platform = $2").bind(user.id).bind(kind.as_str()).execute(&app.db).await?;
        return Ok(Json(json!({ "ok": true })));
    }
    // Turning it on with nowhere to send to would only look like it works.
    let (connected,): (bool,) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM chat_apps WHERE platform = $1)").bind(kind.as_str()).fetch_one(&app.db).await?;
    if !connected {
        return Err(ApiError::bad(format!("{} is not connected. A team admin can connect it under Settings → Chat apps.", kind.title())));
    }
    let (linked,): (bool,) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM chat_identities WHERE platform = $1 AND user_id = $2)")
        .bind(kind.as_str())
        .bind(user.id)
        .fetch_one(&app.db)
        .await?;
    if !linked {
        return Err(ApiError::bad(format!(
            "your {} account is not linked yet. Mention the bot in {} and it will link you or send you a link.",
            kind.title(),
            kind.title()
        )));
    }
    sqlx::query("INSERT INTO notification_channels (user_id, platform) VALUES ($1, $2) ON CONFLICT DO NOTHING").bind(user.id).bind(kind.as_str()).execute(&app.db).await?;
    Ok(Json(json!({ "ok": true })))
}
