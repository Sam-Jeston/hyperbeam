//! Notifications: telling a person that an agent of theirs needs them.
//!
//! Three things are worth someone's attention: an agent asked them a
//! question, finished a turn and is waiting, or failed. Each is recorded for
//! the hub in the web UI and pushed to that person's open browsers. If they
//! have turned a chat app on, its bot also sends them a direct message.
//!
//! Only the person who started an agent is told. A worker under a conductor
//! is quiet: it reports to the conductor, and the conductor's own turns are
//! what its owner hears about.

use crate::app::App;
use crate::chat::{self, Button, Outgoing};
use crate::sessions::clip;
use crate::store::{self, Session};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// Notifications older than this are dropped when the person gets a new one.
const KEEP_DAYS: i32 = 30;
/// How much of a closing message or an error a notification carries.
const BODY_CHARS: usize = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The agent asked the people it works for a question.
    Question,
    /// The agent ended a turn and is waiting to be told what is next.
    Finished,
    Failed,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Question => "question",
            Kind::Finished => "finished",
            Kind::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Notification {
    pub id: Uuid,
    pub session_id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
}

pub const COLUMNS: &str = "id, session_id, kind, title, body, created_at, read_at";

/// The direct message a chat app sends for a notification.
pub fn message(kind: Kind, title: &str, body: &str, url: &str, limit: usize) -> Outgoing {
    let title = title.replace(['*', '\n'], " ");
    let (heading, open) = match kind {
        Kind::Question => (format!("**“{title}” has a question for you**"), "Answer"),
        Kind::Finished => (format!("**“{title}” finished and is waiting for you**"), "Open"),
        Kind::Failed => (format!("**“{title}” failed**"), "Open"),
    };
    let body = clip(body.trim(), limit.saturating_sub(heading.chars().count() + 10).min(1500));
    let text = if body.is_empty() { heading } else { format!("{heading}\n\n{body}") };
    Outgoing { text, buttons: vec![Button::link(open, url)] }
}

impl App {
    /// Does this session answer to a conductor that is still there to hear it?
    pub async fn reports_to_conductor(&self, session: &Session) -> bool {
        let Some(parent_id) = session.parent_id else { return false };
        matches!(
            store::session_by_id(&self.db, parent_id).await,
            Ok(Some(parent)) if !matches!(parent.status.as_str(), "cancelled" | "failed")
        )
    }

    /// Tell the session's owner that it needs them. Never fails the caller:
    /// a notification that could not be made is logged and nothing more.
    pub async fn notify(&self, session: &Session, kind: Kind, body: &str) {
        let body = clip(body.trim(), BODY_CHARS);
        let user_id = session.created_by;
        let made: sqlx::Result<Notification> = sqlx::query_as(&format!(
            "INSERT INTO notifications (id, user_id, session_id, kind, title, body) VALUES ($1, $2, $3, $4, $5, $6) RETURNING {COLUMNS}"
        ))
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(session.id)
        .bind(kind.as_str())
        .bind(&session.title)
        .bind(&body)
        .fetch_one(&self.db)
        .await;
        match made {
            Ok(notification) => {
                self.publish_to(user_id, "notification", serde_json::to_value(&notification).unwrap_or_default());
            }
            Err(err) => tracing::warn!("could not record a notification for session {}: {err}", session.id),
        }
        sqlx::query("DELETE FROM notifications WHERE user_id = $1 AND created_at < now() - make_interval(days => $2)")
            .bind(user_id)
            .bind(KEEP_DAYS)
            .execute(&self.db)
            .await
            .ok();

        // A chat platform may be slow; the session does not wait for it.
        let (app, session) = (self.clone(), session.clone());
        tokio::spawn(async move {
            if let Err(err) = app.notify_by_chat(&session, kind, &body).await {
                tracing::warn!("could not send a chat notification for session {}: {err:#}", session.id);
            }
        });
    }

    /// Direct-message the owner on each chat app they turned on and have an
    /// account linked to.
    async fn notify_by_chat(&self, session: &Session, kind: Kind, body: &str) -> Result<()> {
        let targets: Vec<(String, String)> = sqlx::query_as(
            "SELECT c.platform, i.external_id FROM notification_channels c
             JOIN chat_identities i ON i.platform = c.platform AND i.user_id = c.user_id
             JOIN users u ON u.id = c.user_id AND NOT u.disabled
             WHERE c.user_id = $1
               -- A session with a thread on that platform already speaks there.
               AND NOT EXISTS (SELECT 1 FROM chat_threads t WHERE t.session_id = $2 AND t.platform = c.platform)",
        )
        .bind(session.created_by)
        .bind(session.id)
        .fetch_all(&self.db)
        .await?;
        let url = format!("{}/s/{}", self.cfg.public_url.trim_end_matches('/'), session.id);
        for (platform, external_id) in targets {
            let Some(chat_kind) = chat::Kind::parse(&platform) else { continue };
            // Not connected right now: the notification is still in the hub.
            let Some(client) = self.chat.platform(chat_kind) else { continue };
            let message = message(kind, &session.title, body, &url, client.limit());
            if let Err(err) = client.direct(&external_id, &message).await {
                tracing::warn!("{}: could not notify {external_id}: {err:#}", chat_kind.title());
            }
        }
        Ok(())
    }

    /// Mark a person's notifications as read: one of them, those of one
    /// session, or (given neither) all of them.
    pub async fn read_notifications(&self, user_id: Uuid, id: Option<Uuid>, session_id: Option<Uuid>) -> Result<u64> {
        let read = sqlx::query(
            "UPDATE notifications SET read_at = now()
             WHERE user_id = $1 AND read_at IS NULL AND ($2::uuid IS NULL OR id = $2) AND ($3::uuid IS NULL OR session_id = $3)",
        )
        .bind(user_id)
        .bind(id)
        .bind(session_id)
        .execute(&self.db)
        .await?
        .rows_affected();
        if read > 0 {
            // Their other tabs should agree.
            self.publish_to(user_id, "notifications", serde_json::json!({}));
        }
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_say_what_happened_and_link_to_the_agent() {
        let asked = message(Kind::Question, "Fix checkout", "Postgres or SQLite?", "https://hb.example/s/1", 4000);
        assert_eq!(asked.text, "**“Fix checkout” has a question for you**\n\nPostgres or SQLite?");
        assert_eq!(asked.buttons, vec![Button::link("Answer", "https://hb.example/s/1")]);

        let done = message(Kind::Finished, "Fix *checkout*\nnow", "  ", "u", 4000);
        assert_eq!(done.text, "**“Fix  checkout  now” finished and is waiting for you**");

        let failed = message(Kind::Failed, "Fix checkout", &"x".repeat(5000), "u", 2000);
        assert!(failed.text.starts_with("**“Fix checkout” failed**\n\n"));
        assert!(failed.text.chars().count() <= 2000);
    }
}
