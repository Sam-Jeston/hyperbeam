//! Where agents run. A driver creates one sandbox per live session and tears
//! it down again; the sandbox's runner dials back to the control plane.

pub mod kubernetes;
pub mod process;

use anyhow::Result;
use async_trait::async_trait;
use std::collections::BTreeMap;
use uuid::Uuid;

pub struct LaunchRequest {
    pub session_id: Uuid,
    /// The session's run token: its only credential.
    pub token: String,
    /// Container image the sandbox runs: the harness profile's, or an
    /// environment's (ignored by the process driver).
    pub image: String,
    /// Set when `image` is an environment: the harness image whose agent
    /// (`/opt/harness`) is carried into it.
    pub harness_image: Option<String>,
    /// URL the runner uses to reach the control plane.
    pub server_url: String,
    /// Additional non-secret environment for the sandbox.
    pub env: BTreeMap<String, String>,
}

#[async_trait]
pub trait SandboxDriver: Send + Sync {
    fn name(&self) -> &'static str;
    async fn launch(&self, request: &LaunchRequest) -> Result<()>;
    /// Idempotent: destroying a sandbox that is already gone succeeds.
    async fn destroy(&self, session_id: Uuid) -> Result<()>;
    /// Sessions that currently have a sandbox, for orphan cleanup.
    async fn list(&self) -> Result<Vec<Uuid>>;
}
