//! Shared application state.

use crate::config::Config;
use crate::crypto::SecretBox;
use crate::integrations::github::GithubTokens;
use crate::sandbox::SandboxDriver;
use crate::sessions::LiveSessions;
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

/// Something that happened, fanned out to every connected browser.
#[derive(Debug, Clone, Serialize)]
pub struct StreamMsg {
    /// `event` (one session log entry), `session`, `workstream`, `activity`,
    /// `notification`, `notifications`.
    pub topic: &'static str,
    /// Set when only this person's browsers should be told.
    #[serde(skip)]
    pub to: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
    pub payload: Value,
}

pub struct Inner {
    pub cfg: Config,
    pub db: PgPool,
    pub secrets: SecretBox,
    pub driver: Arc<dyn SandboxDriver>,
    pub live: LiveSessions,
    pub probes: crate::probe::Probes,
    pub bus: broadcast::Sender<Arc<StreamMsg>>,
    pub http: reqwest::Client,
    pub github_tokens: GithubTokens,
    /// Upstream MCP sessions for connections.
    pub mcp: crate::connections::client::McpClient,
    /// Connections whose tools are being re-listed.
    pub refreshing: std::sync::Mutex<std::collections::HashSet<String>>,
    /// OAuth sign-ins an admin has started and not yet finished, by `state`.
    pub oauth_flows: std::sync::Mutex<std::collections::HashMap<String, crate::connections::oauth::Pending>>,
    /// Held while a connection's OAuth token is renewed.
    pub oauth_refresh: tokio::sync::Mutex<()>,
    /// The Slack and Discord apps people start and direct agents from.
    pub chat: crate::chat::Hub,
}

#[derive(Clone)]
pub struct App(Arc<Inner>);

impl Deref for App {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl App {
    pub fn new(cfg: Config, db: PgPool, secrets: SecretBox, driver: Arc<dyn SandboxDriver>) -> Self {
        let (bus, _) = broadcast::channel(4096);
        let http = reqwest::Client::builder()
            .user_agent(concat!("hyperbeam/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("http client");
        App(Arc::new(Inner {
            cfg,
            db,
            secrets,
            driver,
            live: LiveSessions::default(),
            probes: Default::default(),
            bus,
            http,
            github_tokens: GithubTokens::default(),
            mcp: Default::default(),
            refreshing: Default::default(),
            oauth_flows: Default::default(),
            oauth_refresh: Default::default(),
            chat: Default::default(),
        }))
    }

    pub fn publish(&self, topic: &'static str, session_id: Option<Uuid>, payload: Value) {
        let _ = self.bus.send(Arc::new(StreamMsg { topic, to: None, session_id, payload }));
    }

    /// Tell one person's browsers something nobody else should see.
    pub fn publish_to(&self, user_id: Uuid, topic: &'static str, payload: Value) {
        let _ = self.bus.send(Arc::new(StreamMsg { topic, to: Some(user_id), session_id: None, payload }));
    }
}
