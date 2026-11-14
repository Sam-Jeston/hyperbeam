//! Model discovery.
//!
//! A harness only reveals which models it offers once it is running. Rather
//! than wait for somebody's first session, the control plane starts the
//! harness briefly in a sandbox of its own (when it gains a credential, when
//! its image or command changes, or on request), records the list, and tears
//! the sandbox down.
//!
//! A probe is not a session: its token opens the runner endpoint and the
//! egress proxy (for the harness's own hosts) and nothing else. It has no
//! grants, no repositories and no tool gateway.

use crate::app::App;
use crate::crypto;
use crate::sandbox::LaunchRequest;
use crate::sessions::offered_models;
use crate::store::{self, ModelChoice};
use anyhow::{Context, Result, anyhow};
use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use hyperbeam_core::protocol::{HarnessSpec, PROTOCOL_VERSION, RunSpec, RunnerMsg, ServerMsg};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::Duration;
use uuid::Uuid;

/// How long a harness gets to answer once its runner has connected.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone)]
pub struct Probe {
    /// Identifies the sandbox, as a session id would.
    pub id: Uuid,
    pub harness_id: Uuid,
    /// Whose credential the harness starts with (their own, else the team's).
    user_id: Uuid,
    token_hash: String,
}

#[derive(Default)]
pub struct Probes {
    /// At most one probe per harness, keyed by harness id.
    running: Mutex<HashMap<Uuid, Probe>>,
    /// Why the last probe of a harness failed, until one succeeds.
    errors: Mutex<HashMap<Uuid, String>>,
}

impl Probes {
    pub fn by_token(&self, token: &str) -> Option<Probe> {
        let hash = crypto::hash_token(token);
        self.running.lock().unwrap().values().find(|p| p.token_hash == hash).cloned()
    }

    /// Whether a probe of this harness is in flight, and the last failure.
    pub fn status(&self, harness_id: Uuid) -> (bool, Option<String>) {
        let running = self.running.lock().unwrap();
        (running.contains_key(&harness_id), self.errors.lock().unwrap().get(&harness_id).cloned())
    }
}

impl App {
    /// Start the harness in a throwaway sandbox to learn its models. Returns
    /// whether a probe is now running; it is not when the harness still lacks
    /// a credential it needs.
    pub async fn discover_models(&self, harness_id: Uuid, user_id: Uuid) -> bool {
        match self.start_probe(harness_id, user_id).await {
            Ok(started) => started,
            Err(err) => {
                tracing::warn!("model discovery for harness {harness_id} could not start: {err:#}");
                self.probes.errors.lock().unwrap().insert(harness_id, format!("{err:#}"));
                self.publish("harness", None, json!({ "id": harness_id }));
                false
            }
        }
    }

    async fn start_probe(&self, harness_id: Uuid, user_id: Uuid) -> Result<bool> {
        let Some(harness) = store::harness_by_id(&self.db, harness_id).await? else { return Ok(false) };
        if !harness.credentials.is_empty() && self.harness_credential(&harness, user_id).await?.is_none() {
            return Ok(false);
        }
        let token = crypto::new_token("nmr");
        let probe = Probe { id: Uuid::now_v7(), harness_id, user_id, token_hash: crypto::hash_token(&token) };
        // A newer request replaces a probe in flight: the credential or the
        // profile it started with may be the thing that just changed.
        let replaced = self.probes.running.lock().unwrap().insert(harness_id, probe.clone());
        if let Some(old) = replaced {
            self.driver.destroy(old.id).await.ok();
        }
        self.probes.errors.lock().unwrap().remove(&harness_id);

        let request = LaunchRequest {
            session_id: probe.id,
            token,
            image: harness.image.clone(),
            harness_image: None,
            server_url: self.cfg.internal_url.clone(),
            env: BTreeMap::new(),
        };
        if let Err(err) = self.driver.launch(&request).await {
            self.finish_probe(&probe, Err(format!("could not start a sandbox: {err:#}"))).await;
            return Ok(true);
        }

        // Nothing else watches a probe, so bound its life here.
        let app = self.clone();
        let limit = Duration::from_secs(self.cfg.start_timeout_secs) + ANSWER_TIMEOUT;
        tokio::spawn(async move {
            tokio::time::sleep(limit).await;
            app.finish_probe(&probe, Err("the harness did not report its models in time".into())).await;
        });
        Ok(true)
    }

    /// Record a probe's outcome and remove its sandbox. Only the first call
    /// for a probe has any effect.
    async fn finish_probe(&self, probe: &Probe, outcome: Result<Vec<ModelChoice>, String>) {
        let current = |running: &HashMap<Uuid, Probe>| running.get(&probe.harness_id).is_some_and(|p| p.id == probe.id);
        if !current(&self.probes.running.lock().unwrap()) {
            return; // already finished, or replaced by a newer probe
        }
        // Save the list before the probe stops counting as running, so nobody
        // watching sees "finished" without the result.
        let outcome = match outcome {
            Ok(models) if !models.is_empty() => sqlx::query("UPDATE harnesses SET discovered_models = $2 WHERE id = $1")
                .bind(probe.harness_id)
                .bind(json!(models))
                .execute(&self.db)
                .await
                .map(|_| ())
                .map_err(|err| err.to_string()),
            // A harness without a model option is not an error; there is
            // simply nothing to choose from.
            Ok(_) => Ok(()),
            Err(err) => Err(err),
        };
        {
            let mut running = self.probes.running.lock().unwrap();
            if !current(&running) {
                return;
            }
            if let Err(err) = outcome {
                tracing::warn!("model discovery for harness {} failed: {err}", probe.harness_id);
                self.probes.errors.lock().unwrap().insert(probe.harness_id, err);
            }
            running.remove(&probe.harness_id);
        }
        if let Err(err) = self.driver.destroy(probe.id).await {
            tracing::warn!("could not destroy the model discovery sandbox {}: {err:#}", probe.id);
        }
        self.publish("harness", None, json!({ "id": probe.harness_id }));
    }

    async fn probe_spec(&self, probe: &Probe, token: &str) -> Result<RunSpec> {
        let harness = store::harness_by_id(&self.db, probe.harness_id).await?.ok_or_else(|| anyhow!("harness profile was deleted"))?;
        let mut env = harness.env.0.clone();
        if let Some((name, value)) = self.harness_credential(&harness, probe.user_id).await? {
            env.insert(name, value);
        }
        self.proxy_env(token, &mut env);
        let base = self.cfg.internal_url.trim_end_matches('/');
        Ok(RunSpec {
            session_id: probe.id,
            title: format!("Model discovery for {}", harness.title),
            harness: HarnessSpec { name: harness.name, command: harness.command.0, env, model: None },
            repos: Vec::new(),
            skills: Vec::new(),
            mcp_url: format!("{base}/mcp"),
            git_base_url: format!("{base}/git"),
            preamble: String::new(),
            git_user_name: String::new(),
            git_user_email: String::new(),
            work_branch: String::new(),
            probe: true,
        })
    }
}

/// Serve the runner of a probe sandbox: hand it a probe spec, take the
/// harness's answer, and end the probe.
pub async fn serve(app: App, probe: Probe, token: String, mut socket: WebSocket) {
    let outcome = match tokio::time::timeout(ANSWER_TIMEOUT, converse(&app, &probe, &token, &mut socket)).await {
        Ok(Ok(models)) => Ok(models),
        Ok(Err(err)) => Err(format!("{err:#}")),
        Err(_) => Err("the harness did not report its models in time".to_string()),
    };
    if let Ok(text) = serde_json::to_string(&ServerMsg::Shutdown) {
        let _ = socket.send(Message::Text(text.into())).await;
    }
    let _ = socket.close().await;
    app.finish_probe(&probe, outcome).await;
}

async fn converse(app: &App, probe: &Probe, token: &str, socket: &mut WebSocket) -> Result<Vec<ModelChoice>> {
    let mut greeted = false;
    while let Some(frame) = socket.next().await {
        let text = match frame.context("the sandbox connection failed")? {
            Message::Text(text) => text,
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(msg) = serde_json::from_str::<RunnerMsg>(&text) else { continue };
        match msg {
            RunnerMsg::Hello { protocol, .. } if !greeted => {
                if protocol != PROTOCOL_VERSION {
                    anyhow::bail!("the runner speaks protocol {protocol}, expected {PROTOCOL_VERSION}");
                }
                greeted = true;
                let spec = app.probe_spec(probe, token).await?;
                let text = serde_json::to_string(&ServerMsg::Spec { spec: Box::new(spec) })?;
                socket.send(Message::Text(text.into())).await.context("the sandbox connection failed")?;
            }
            RunnerMsg::Ready { agent } => return Ok(offered_models(&agent)),
            RunnerMsg::Failed { error } => anyhow::bail!("{error}"),
            _ => {}
        }
    }
    Err(anyhow!("the sandbox disconnected before the harness answered"))
}
