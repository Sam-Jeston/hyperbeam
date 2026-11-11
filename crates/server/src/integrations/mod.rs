//! Credentials for the external systems agents reach through the gateway.
//! They are held here, in the control plane, and never enter a sandbox.

pub mod github;

use crate::app::App;
use crate::store;
use anyhow::Result;
use serde_json::Value;

/// Built-in integrations. Everything else is an MCP connection.
pub const KINDS: [&str; 2] = ["github", "notion"];

pub fn secret_key(kind: &str) -> String {
    format!("integration/{kind}")
}

impl App {
    pub async fn integration_config(&self, kind: &str) -> Result<Option<Value>> {
        let row: Option<(Value,)> =
            sqlx::query_as("SELECT config FROM integrations WHERE kind = $1").bind(kind).fetch_optional(&self.db).await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn integration_secret(&self, kind: &str) -> Result<Option<String>> {
        if self.integration_config(kind).await?.is_none() {
            return Ok(None);
        }
        store::get_secret(&self.db, &self.secrets, &secret_key(kind)).await
    }
}
