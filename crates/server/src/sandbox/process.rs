//! Development driver: each agent is a child process on the server's host.
//!
//! There is no isolation here. An agent can read anything the server's user
//! can. Use it to try Hyperbeam on a laptop; use Kubernetes for anything shared.

use super::{LaunchRequest, SandboxDriver};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct ProcessDriver {
    runner_bin: PathBuf,
    root: PathBuf,
    children: Mutex<HashMap<Uuid, Child>>,
}

impl ProcessDriver {
    pub fn new(runner_bin: Option<PathBuf>, root: Option<PathBuf>) -> Result<Self> {
        let runner_bin = match runner_bin {
            Some(path) => path,
            None => std::env::current_exe()?
                .parent()
                .context("cannot locate the server binary's directory")?
                .join("hyperbeam-runner"),
        };
        if !runner_bin.exists() {
            anyhow::bail!(
                "runner binary not found at {} (build it with `cargo build -p hyperbeam-runner` or set HYPERBEAM_RUNNER_BIN)",
                runner_bin.display()
            );
        }
        let root = root.unwrap_or_else(|| std::env::temp_dir().join("hyperbeam-sandboxes"));
        std::fs::create_dir_all(&root)?;
        Ok(Self { runner_bin, root, children: Mutex::default() })
    }

    fn dir(&self, session_id: Uuid) -> PathBuf {
        self.root.join(session_id.to_string())
    }
}

#[async_trait]
impl SandboxDriver for ProcessDriver {
    fn name(&self) -> &'static str {
        "process"
    }

    async fn launch(&self, request: &LaunchRequest) -> Result<()> {
        self.destroy(request.session_id).await.ok();
        let dir = self.dir(request.session_id);
        let home = dir.join("home");
        tokio::fs::create_dir_all(dir.join("workspace")).await?;
        tokio::fs::create_dir_all(&home).await?;
        let log = std::fs::File::create(dir.join("runner.log"))?;

        // The child must not inherit the server's environment: it holds the
        // database URL, and the master key or the AWS credentials that reach it.
        let child = Command::new(&self.runner_bin)
            .arg("run")
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("LANG", "C.UTF-8")
            .env("HYPERBEAM_SERVER_URL", &request.server_url)
            .env("HYPERBEAM_RUN_TOKEN", &request.token)
            .env("HYPERBEAM_WORKSPACE", dir.join("workspace"))
            .envs(&request.env)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("could not start {}", self.runner_bin.display()))?;
        self.children.lock().await.insert(request.session_id, child);
        Ok(())
    }

    async fn destroy(&self, session_id: Uuid) -> Result<()> {
        if let Some(mut child) = self.children.lock().await.remove(&session_id) {
            // Give the runner a moment to exit on its own after Shutdown.
            if tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await.is_err() {
                let _ = child.kill().await;
            }
        }
        let dir = self.dir(session_id);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await.ok();
        }
        Ok(())
    }

    async fn list(&self) -> Result<Vec<Uuid>> {
        Ok(self.children.lock().await.keys().copied().collect())
    }
}
