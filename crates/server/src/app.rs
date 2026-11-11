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
    /// `event` (one session log entry), `session`, `workstream`, `activity`.
    pub topic: &'static str,
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
        }))
    }

    pub fn publish(&self, topic: &'static str, session_id: Option<Uuid>, payload: Value) {
        let _ = self.bus.send(Arc::new(StreamMsg { topic, session_id, payload }));
    }
}
