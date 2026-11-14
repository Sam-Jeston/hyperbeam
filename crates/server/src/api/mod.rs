//! The HTTP API used by the web UI and the CLI.

mod account;
mod chat;
mod cluster;
mod connections;
mod notifications;
mod settings;
mod work;

use crate::app::{App, StreamMsg};
use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::store;
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use futures::Stream;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use std::convert::Infallible;
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

/// Distinguishes an absent field from an explicit `null` in PATCH bodies.
pub fn nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

pub fn router() -> Router<App> {
    Router::new()
        .route("/meta", get(meta))
        .route("/stream", get(stream_all))
        // account
        .route("/setup", post(account::setup))
        .route("/auth/login", post(account::login))
        .route("/auth/logout", post(account::logout))
        .route("/auth/accept-invite", post(account::accept_invite))
        .route("/me", get(account::me).patch(account::update_me))
        .route("/me/api-keys", get(account::list_api_keys).post(account::create_api_key))
        .route("/me/api-keys/{id}", delete(account::delete_api_key))
        .route("/users", get(account::list_users))
        .route("/users/{id}", patch(account::update_user))
        .route("/invites", get(account::list_invites).post(account::create_invite))
        .route("/invites/{id}", delete(account::delete_invite))
        .route("/invite-info/{token}", get(account::invite_info))
        // configuration
        .route("/settings", get(settings::get_settings).put(settings::put_settings))
        .route("/integrations", get(settings::list_integrations))
        .route("/integrations/{kind}", put(settings::put_integration).delete(settings::delete_integration))
        .route("/integrations/{kind}/test", post(settings::test_integration))
        .route("/connections", get(connections::list).post(connections::create))
        .route("/connections/{name}", patch(connections::update).delete(connections::delete))
        .route("/connections/{name}/refresh", post(connections::refresh))
        .route("/connections/{name}/authorize", post(connections::authorize))
        .route("/oauth/callback", get(connections::oauth_callback))
        .route("/connections/{name}/approvals", put(connections::set_approvals))
        .route("/grants", get(connections::grants))
        .route("/chat", get(chat::list))
        .route("/chat/link/{code}", get(chat::link_info).post(chat::link))
        .route("/chat/{platform}", put(chat::put).delete(chat::delete))
        .route("/chat/{platform}/channels", get(chat::channels))
        .route("/chat/{platform}/channels/{id}", put(chat::put_channel))
        .route("/me/chat-identities", get(chat::my_identities))
        .route("/me/chat-identities/{platform}/{id}", delete(chat::unlink))
        .route("/notifications", get(notifications::list))
        .route("/notifications/read", post(notifications::read))
        .route("/me/notification-channels", get(notifications::channels))
        .route("/me/notification-channels/{platform}", put(notifications::put_channel))
        .route("/repos", get(settings::list_repos).post(settings::add_repo))
        .route("/repos/{id}", patch(settings::update_repo).delete(settings::delete_repo))
        .route("/environments", get(settings::list_environments).post(settings::create_environment))
        .route("/environments/{id}", patch(settings::update_environment).delete(settings::delete_environment))
        .route("/harnesses", get(settings::list_harnesses).post(settings::create_harness))
        .route("/harnesses/{id}", patch(settings::update_harness).delete(settings::delete_harness))
        .route("/harnesses/{id}/credential", put(settings::put_credential).delete(settings::delete_credential))
        .route("/harnesses/{id}/models/refresh", post(settings::refresh_models))
        .route("/skills", get(settings::list_skills).post(settings::create_skill))
        .route("/skills/{id}", put(settings::update_skill).delete(settings::delete_skill))
        .route("/workflows", get(settings::list_workflows).post(settings::create_workflow))
        .route("/workflows/{id}", put(settings::update_workflow).delete(settings::delete_workflow))
        .route("/workflows/{id}/run", post(work::run_workflow))
        .route("/memory", get(settings::list_memory))
        .route("/memory/{key}", put(settings::put_memory).delete(settings::delete_memory))
        // work
        .route("/workstreams", get(work::list_workstreams).post(work::create_workstream))
        .route("/workstreams/{id}", get(work::get_workstream).patch(work::update_workstream))
        .route("/workstreams/{id}/comments", post(work::add_comment))
        .route("/sessions", get(work::list_sessions).post(work::create_session))
        .route("/sessions/{id}", get(work::get_session))
        .route("/sessions/{id}/events", get(work::list_events))
        .route("/sessions/{id}/stream", get(stream_session))
        .route("/sessions/{id}/messages", post(work::send_message))
        .route("/sessions/{id}/answer", post(work::answer_question))
        .route("/sessions/{id}/cancel", post(work::cancel_session))
        .route("/sessions/{id}/stop", post(work::stop_session))
        .route("/activity", get(work::list_activity))
        .route("/audit", get(work::list_audit))
        .route("/cluster", get(cluster::get))
}

/// Public: what the login screen needs before anyone is signed in.
async fn meta(State(app): State<App>) -> ApiResult<Json<Value>> {
    let (users,): (i64,) = sqlx::query_as("SELECT count(*) FROM users").fetch_one(&app.db).await?;
    let team_name = store::get_setting(&app.db, "team_name").await?.and_then(|v| v.as_str().map(str::to_string));
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "needs_setup": users == 0,
        "team_name": team_name,
        "sandbox_driver": app.driver.name(),
    })))
}

fn sse_frame(msg: &StreamMsg) -> SseEvent {
    SseEvent::default().event(msg.topic).data(msg.payload.to_string())
}

/// Team-wide stream: session, workstream and activity changes, and the
/// signed-in person's own notifications.
async fn stream_all(State(app): State<App>, AuthUser(user): AuthUser) -> Response {
    let mut rx = app.bus.subscribe();
    let stream = async_stream(move |tx| async move {
        loop {
            match rx.recv().await {
                Ok(msg) if msg.topic != "event" => {
                    // Some things are for one person only.
                    if msg.to.is_some_and(|to| to != user.id) {
                        continue;
                    }
                    if tx.send(sse_frame(&msg)).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                // Fell behind: tell the client to refetch rather than guess.
                Err(RecvError::Lagged(_)) => {
                    if tx.send(SseEvent::default().event("resync").data("{}")).is_err() {
                        break;
                    }
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

#[derive(Deserialize)]
struct StreamParams {
    #[serde(default)]
    after: i64,
}

/// One session's log: everything after `after`, then live.
async fn stream_session(
    State(app): State<App>,
    _user: AuthUser,
    Path(id): Path<Uuid>,
    Query(params): Query<StreamParams>,
) -> ApiResult<Response> {
    store::session_by_id(&app.db, id).await?.ok_or(crate::error::ApiError::NotFound("session"))?;
    // Subscribe before reading history so nothing falls in the gap.
    let mut rx = app.bus.subscribe();
    let stream = async_stream(move |tx| async move {
        let mut last = params.after;
        loop {
            let backlog: Vec<store::Event> = match sqlx::query_as(
                "SELECT session_id, seq, ts, kind, data FROM session_events WHERE session_id = $1 AND seq > $2 ORDER BY seq LIMIT 1000",
            )
            .bind(id)
            .bind(last)
            .fetch_all(&app.db)
            .await
            {
                Ok(rows) => rows,
                Err(_) => return,
            };
            let done = backlog.len() < 1000;
            for event in backlog {
                last = event.seq;
                let frame = SseEvent::default().event("event").data(serde_json::to_string(&event).unwrap_or_default());
                if tx.send(frame).is_err() {
                    return;
                }
            }
            if done {
                break;
            }
        }
        let _ = tx.send(SseEvent::default().event("caught_up").data("{}"));
        loop {
            match rx.recv().await {
                Ok(msg) if msg.session_id == Some(id) => {
                    if msg.topic == "event" {
                        let seq = msg.payload.get("seq").and_then(Value::as_i64).unwrap_or(0);
                        if seq <= last {
                            continue;
                        }
                        last = seq;
                    }
                    if tx.send(sse_frame(&msg)).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => {
                    if tx.send(SseEvent::default().event("resync").data("{}")).is_err() {
                        break;
                    }
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()).into_response())
}

/// Run a producer task and expose what it sends as an SSE stream.
fn async_stream<F, Fut>(producer: F) -> std::pin::Pin<Box<dyn Stream<Item = Result<SseEvent, Infallible>> + Send>>
where
    F: FnOnce(tokio::sync::mpsc::UnboundedSender<SseEvent>) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(producer(tx));
    Box::pin(futures::StreamExt::map(tokio_stream::wrappers::UnboundedReceiverStream::new(rx), Ok))
}
