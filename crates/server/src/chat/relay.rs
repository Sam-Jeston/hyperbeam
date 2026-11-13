//! Keeps chat threads current with their sessions.
//!
//! A thread is not a transcript: the full log is on the session's page. It
//! gets one status message, edited in place, then what the agent says at the
//! end of each turn, its questions, and errors. Each thread remembers how far
//! through the session's event log it has relayed, so nothing is lost or
//! repeated across a restart.

use super::{Button, Kind, Outgoing, Platform, Style, THREAD_COLUMNS, Thread, text};
use crate::app::App;
use crate::sessions::clip;
use crate::store::{self, Session};
use anyhow::Result;
use hyperbeam_core::protocol::{SessionStatus, event};
use serde_json::Value;
use sqlx::types::Json;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use uuid::Uuid;

/// The event kinds a thread is told about.
const RELAYED: [&str; 3] = [event::TURN_END, event::QUESTION, event::ERROR];

pub async fn run(app: App, mut dirty: mpsc::UnboundedReceiver<Uuid>) {
    // Watch the bus for sessions with a thread, and queue them.
    {
        let app = app.clone();
        let mut bus = app.bus.subscribe();
        tokio::spawn(async move {
            loop {
                match bus.recv().await {
                    Ok(msg) => {
                        let Some(id) = msg.session_id.filter(|id| app.chat.is_bound(*id)) else { continue };
                        let relevant = match msg.topic {
                            "event" => msg.payload["kind"].as_str().is_some_and(|kind| RELAYED.contains(&kind)),
                            // A full session row, not just a new headline.
                            "session" => msg.payload.get("status").is_some(),
                            _ => false,
                        };
                        if relevant {
                            app.chat.touch(id);
                        }
                    }
                    // Fell behind: look at every thread rather than guess.
                    Err(RecvError::Lagged(_)) => {
                        let all: Vec<Uuid> = app.chat.bound.lock().unwrap().iter().copied().collect();
                        for id in all {
                            app.chat.touch(id);
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }
    while let Some(session_id) = dirty.recv().await {
        if let Err(err) = sync(&app, session_id).await {
            tracing::warn!("chat relay for session {session_id}: {err:#}");
        }
    }
}

/// A digest of a message, to tell whether it would change what is shown.
fn digest(message: &Outgoing) -> String {
    crate::crypto::hash_token(&format!("{message:?}"))
}

async fn sync(app: &App, session_id: Uuid) -> Result<()> {
    let threads: Vec<Thread> = sqlx::query_as(&format!("SELECT {THREAD_COLUMNS} FROM chat_threads WHERE session_id = $1")).bind(session_id).fetch_all(&app.db).await?;
    let Some(session) = store::session_by_id(&app.db, session_id).await? else { return Ok(()) };
    for thread in threads {
        let Some(kind) = Kind::parse(&thread.platform) else { continue };
        let Some(platform) = app.chat.platform(kind) else { continue };
        // The status first, so a new thread opens with it. It is edited only
        // when what it says changes.
        let status = status_message(app, &session);
        let shown = digest(&status);
        if thread.status_shown.as_deref() != Some(shown.as_str()) {
            let place = thread.place();
            let id = match &thread.status_message_id {
                Some(id) => {
                    platform.update(&place, id, &status).await?;
                    id.clone()
                }
                None => platform.post(&place, &status).await?,
            };
            sqlx::query("UPDATE chat_threads SET status_message_id = $2, status_shown = $3 WHERE id = $1").bind(thread.id).bind(&id).bind(&shown).execute(&app.db).await?;
        }
        relay_events(app, kind, platform.as_ref(), &session, &thread).await?;
    }
    Ok(())
}

/// Post what happened since the thread was last brought up to date. The
/// mark moves after each post, so a failure is retried from where it stopped.
async fn relay_events(app: &App, kind: Kind, platform: &dyn Platform, session: &Session, thread: &Thread) -> Result<()> {
    let events: Vec<(i64, String, Json<Value>)> = sqlx::query_as(
        "SELECT seq, kind, data FROM session_events WHERE session_id = $1 AND seq > $2 AND kind = ANY($3) ORDER BY seq",
    )
    .bind(session.id)
    .bind(thread.last_seq)
    .bind(&RELAYED[..])
    .fetch_all(&app.db)
    .await?;
    let place = thread.place();
    for (seq, event_kind, Json(data)) in events {
        let mut posts: Vec<Outgoing> = Vec::new();
        match event_kind.as_str() {
            event::TURN_END => {
                let said = closing_prose(app, session.id, seq).await?;
                if !said.trim().is_empty() {
                    posts.extend(text::chunks(&said, platform.limit()).into_iter().map(Outgoing::text));
                } else if data["stop_reason"] == "cancelled" {
                    posts.push(Outgoing::text("Interrupted."));
                }
            }
            event::QUESTION if data["to"] != "conductor" => {
                let question = data["text"].as_str().unwrap_or_default();
                let id = data["id"].as_str().unwrap_or_default();
                let owner = owner_mention(app, kind, session.created_by).await?;
                let options = data["options"].as_array().cloned().unwrap_or_default();
                let buttons = options
                    .iter()
                    .filter_map(Value::as_str)
                    .enumerate()
                    .take(20)
                    .map(|(i, option)| Button::new(format!("answer:{id}:{i}"), clip(option, 75)))
                    .collect::<Vec<_>>();
                let how = if buttons.is_empty() { "Reply here to answer." } else { "Choose one, or reply here in your own words." };
                let body = clip(question, platform.limit().saturating_sub(200));
                posts.push(Outgoing { text: format!("{owner}**Question:** {body}\n\n{how}"), buttons });
            }
            event::ERROR => {
                if let Some(error) = data["text"].as_str().filter(|e| !e.trim().is_empty()) {
                    posts.push(Outgoing::text(format!("**Error:** {}", clip(error, platform.limit().saturating_sub(50)))));
                }
            }
            _ => {}
        }
        for post in &posts {
            platform.post(&place, post).await?;
        }
        sqlx::query("UPDATE chat_threads SET last_seq = $2 WHERE id = $1").bind(thread.id).bind(seq).execute(&app.db).await?;
    }
    Ok(())
}

/// What the agent said to close the turn that ended at `turn_end`: its prose
/// after its last tool call or, if it said nothing then, all it said that turn.
async fn closing_prose(app: &App, session_id: Uuid, turn_end: i64) -> Result<String> {
    for boundary in [&["turn_end", "tool_call", "tool_call_update"][..], &["turn_end"][..]] {
        let (said,): (Option<String>,) = sqlx::query_as(
            "SELECT string_agg(data->>'text', '' ORDER BY seq) FROM session_events
             WHERE session_id = $1 AND kind = 'agent_message' AND seq < $2
               AND seq > (SELECT COALESCE(MAX(seq), 0) FROM session_events
                          WHERE session_id = $1 AND seq < $2 AND kind = ANY($3))",
        )
        .bind(session_id)
        .bind(turn_end)
        .bind(boundary)
        .fetch_one(&app.db)
        .await?;
        if let Some(said) = said.filter(|s| !s.trim().is_empty()) {
            return Ok(said);
        }
    }
    Ok(String::new())
}

/// A mention of the session's owner, so a question reaches them.
async fn owner_mention(app: &App, kind: Kind, owner: Uuid) -> Result<String> {
    let id: Option<(String,)> = sqlx::query_as("SELECT external_id FROM chat_identities WHERE platform = $1 AND user_id = $2 ORDER BY linked_at DESC LIMIT 1")
        .bind(kind.as_str())
        .bind(owner)
        .fetch_optional(&app.db)
        .await?;
    Ok(id.map(|(id,)| format!("<@{id}> ")).unwrap_or_default())
}

/// The one message that says where the session stands.
pub fn status_message(app: &App, session: &Session) -> Outgoing {
    let status = SessionStatus::parse(&session.status).unwrap_or(SessionStatus::Failed);
    let line = match status {
        _ if session.question.is_some() && status != SessionStatus::Running => "Has a question for you.".to_string(),
        SessionStatus::Pending | SessionStatus::Starting => "Starting…".to_string(),
        SessionStatus::Running => "Working…".to_string(),
        SessionStatus::Idle => "Waiting for your reply.".to_string(),
        SessionStatus::Completed => "Asleep. Reply here to wake it.".to_string(),
        SessionStatus::Failed => format!("Failed: {}", clip(session.error.as_deref().unwrap_or("unknown error"), 300)),
        SessionStatus::Cancelled => "Stopped. Reply here to start it again.".to_string(),
    };
    let mut text = format!("**{}**\n{line}", session.title);
    if !session.repos.is_empty() {
        let repos: Vec<String> = session.repos.iter().map(|repo| format!("`{repo}`")).collect();
        text.push_str(&format!("\n{} · branch `{}`", repos.join(", "), session.work_branch));
    }
    for link in session.links.iter() {
        text.push_str(&format!("\n[{}]({})", link.title.replace(['[', ']'], ""), link.url));
    }
    let url = format!("{}/s/{}", app.cfg.public_url.trim_end_matches('/'), session.id);
    let mut buttons = vec![Button::link("Watch", url)];
    if status == SessionStatus::Running {
        buttons.push(Button::new("interrupt", "Interrupt"));
    }
    if status.is_live() {
        buttons.push(Button::new("stop", "Stop").styled(Style::Danger));
    }
    Outgoing { text, buttons }
}
