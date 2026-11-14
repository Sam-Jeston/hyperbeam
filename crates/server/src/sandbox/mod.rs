//! Where agents run. A driver creates one sandbox per live session and tears
//! it down again; the sandbox's runner dials back to the control plane.

pub mod kubernetes;
pub mod process;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
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
    /// What can be seen of where sandboxes run, for the Cluster page. Never
    /// fails as a whole: each part that could not be read says why.
    async fn inspect(&self) -> Inspection;
    /// Fetch `image` afresh onto every ready node, so the next sandbox that
    /// starts from it runs what its tag names now. Returns once the pulls are
    /// under way, with how many nodes they are on; `inspect` reports progress.
    async fn pull(&self, _image: &str) -> Result<usize> {
        anyhow::bail!("this driver runs no images")
    }
}

/// A driver's view of where sandboxes run, as far as it is allowed to look.
#[derive(Debug, Default, Serialize)]
pub struct Inspection {
    /// Kubernetes: the namespace sandboxes run in.
    pub namespace: Option<String>,
    pub nodes: Vec<NodeInfo>,
    pub nodes_error: Option<String>,
    pub sandboxes: Vec<SandboxInfo>,
    pub sandboxes_error: Option<String>,
    /// Newest first.
    pub events: Vec<EventInfo>,
    pub events_error: Option<String>,
    /// Image pulls asked for from the Cluster page, the latest per image and node.
    pub pulls: Vec<PullInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PullState {
    Pulling,
    Pulled,
    Failed,
}

#[derive(Debug, Serialize)]
pub struct PullInfo {
    /// As it was asked for.
    pub image: String,
    pub node: String,
    pub state: PullState,
    /// Why it failed, when it did.
    pub message: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default, Serialize)]
pub struct NodeInfo {
    pub name: String,
    pub ready: bool,
    /// Not cordoned.
    pub schedulable: bool,
    pub roles: Vec<String>,
    pub arch: String,
    pub os: String,
    pub kubelet: String,
    pub runtime: String,
    /// Allocatable CPU, as Kubernetes writes it (`4`, `3900m`).
    pub cpu: String,
    /// Allocatable memory.
    pub memory_bytes: Option<u64>,
    /// Conditions that are not as they should be, in words.
    pub problems: Vec<String>,
    pub created_at: Option<DateTime<Utc>>,
    /// The images the node says it holds.
    #[serde(skip)]
    pub images: Vec<NodeImage>,
    /// False when the node may hold more images than it reports, so an
    /// image it does not list is not known to be absent.
    pub images_complete: bool,
}

#[derive(Debug, Default, Clone)]
pub struct NodeImage {
    pub names: Vec<String>,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxState {
    /// Scheduled or being prepared: pulling images, running init steps.
    Starting,
    Running,
    /// Stuck or broken: an image that cannot be pulled, a crash, no node to run on.
    Failing,
    Finished,
    Terminating,
}

#[derive(Debug, Serialize)]
pub struct SandboxInfo {
    /// The session (or model discovery probe) it belongs to.
    pub id: Option<Uuid>,
    pub name: String,
    pub node: Option<String>,
    pub state: SandboxState,
    /// A short machine reason (`ImagePullBackOff`, `PodInitializing`), if any.
    pub reason: String,
    pub message: String,
    pub image: String,
    pub restarts: i32,
    pub created_at: Option<DateTime<Utc>>,
    pub containers: Vec<ContainerInfo>,
}

#[derive(Debug, Serialize)]
pub struct ContainerInfo {
    pub name: String,
    pub image: String,
    pub init: bool,
    /// `waiting`, `running` or `terminated`.
    pub state: &'static str,
    pub reason: String,
    pub message: String,
    pub exit_code: Option<i32>,
}

impl ContainerInfo {
    /// The image could not be fetched or named.
    pub fn pull_failed(&self) -> bool {
        matches!(self.reason.as_str(), "ErrImagePull" | "ImagePullBackOff" | "InvalidImageName" | "ErrImageNeverPull")
    }
}

#[derive(Debug, Serialize)]
pub struct EventInfo {
    pub ts: Option<DateTime<Utc>>,
    pub warning: bool,
    pub reason: String,
    /// What it is about, such as `Pod/agent-…`.
    pub object: String,
    pub message: String,
    pub count: i32,
}

/// An image reference as container runtimes spell it in full, so that
/// `rust:1.90` and `docker.io/library/rust:1.90` compare equal.
pub fn normalize_image(reference: &str) -> String {
    let reference = reference.trim();
    let first = reference.split('/').next().unwrap_or_default();
    let has_registry = reference.contains('/') && (first.contains('.') || first.contains(':') || first == "localhost");
    let mut name = if has_registry {
        match reference.strip_prefix("index.docker.io/") {
            Some(rest) => format!("docker.io/{rest}"),
            None => reference.to_string(),
        }
    } else if reference.contains('/') {
        format!("docker.io/{reference}")
    } else {
        format!("docker.io/library/{reference}")
    };
    let last = name.rsplit('/').next().unwrap_or_default();
    if !last.contains(':') && !last.contains('@') {
        name.push_str(":latest");
    }
    name
}

#[cfg(test)]
mod tests {
    use super::normalize_image;

    #[test]
    fn image_references_are_compared_in_full() {
        assert_eq!(normalize_image("rust:1.90"), "docker.io/library/rust:1.90");
        assert_eq!(normalize_image("rust"), "docker.io/library/rust:latest");
        assert_eq!(normalize_image("acme/dev"), "docker.io/acme/dev:latest");
        assert_eq!(normalize_image("index.docker.io/acme/dev:2"), "docker.io/acme/dev:2");
        assert_eq!(normalize_image("ghcr.io/acme/dev"), "ghcr.io/acme/dev:latest");
        assert_eq!(normalize_image("localhost:5000/dev:1"), "localhost:5000/dev:1");
        assert_eq!(normalize_image("ghcr.io/acme/dev@sha256:abc"), "ghcr.io/acme/dev@sha256:abc");
    }
}
