//! The runner's main loop: one harness process, one ACP session, driven by
//! prompts from the control plane and reporting everything it does.

use crate::acp::{self, AcpClient, Incoming};
use crate::conn::{self, Link};
use crate::workspace;
use anyhow::{Context, Result, anyhow, bail};
use hyperbeam_core::protocol::{RunSpec, RunnerMsg, ServerMsg, event};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct Config {
    pub server_url: String,
    pub token: String,
    pub workspace: PathBuf,
}

/// Longest single string kept in an event. Tool output can be megabytes; the
/// session log is for people to follow, not an artifact store.
const MAX_EVENT_STRING: usize = 16 * 1024;
const FLUSH_INTERVAL: Duration = Duration::from_millis(250);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(300);

pub async fn run(cfg: Config) -> Result<()> {
    let mut link = conn::connect(&cfg.server_url, &cfg.token).await?;
    let spec = loop {
        match link.rx.recv().await {
            Some(ServerMsg::Spec { spec }) => break *spec,
            Some(ServerMsg::Shutdown) | None => return Ok(()),
            Some(_) => continue,
        }
    };
    let result = drive(&cfg, &spec, &mut link).await;
    if let Err(err) = &result {
        let _ = link.tx.send(RunnerMsg::Failed { error: format!("{err:#}") });
    }
    link.close().await;
    result
}

struct Reporter {
    tx: mpsc::UnboundedSender<RunnerMsg>,
    /// Streamed text waiting to be sent as one event.
    buffer: Option<(&'static str, String)>,
    /// The agent's prose during the current turn, for the turn summary.
    turn_text: String,
}

impl Reporter {
    fn event(&mut self, kind: &str, mut data: Value) {
        self.flush();
        truncate_strings(&mut data, MAX_EVENT_STRING);
        let _ = self.tx.send(RunnerMsg::Event { kind: kind.to_string(), data });
    }

    fn system(&mut self, text: impl Into<String>) {
        self.event(event::SYSTEM, json!({ "text": text.into() }));
    }

    fn text(&mut self, kind: &'static str, chunk: &str) {
        if kind == event::AGENT_MESSAGE {
            self.turn_text.push_str(chunk);
        }
        match &mut self.buffer {
            Some((k, buf)) if *k == kind => buf.push_str(chunk),
            _ => {
                self.flush();
                self.buffer = Some((kind, chunk.to_string()));
            }
        }
        if self.buffer.as_ref().is_some_and(|(_, b)| b.len() > 4096) {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if let Some((kind, text)) = self.buffer.take() {
            if !text.is_empty() {
                let _ = self.tx.send(RunnerMsg::Event { kind: kind.to_string(), data: json!({ "text": text }) });
            }
        }
    }
}

pub fn truncate_strings(value: &mut Value, max: usize) {
    match value {
        Value::String(s) if s.len() > max => {
            let mut cut = max;
            while !s.is_char_boundary(cut) {
                cut -= 1;
            }
            let dropped = s.len() - cut;
            s.truncate(cut);
            s.push_str(&format!("\n… [{dropped} bytes truncated]"));
        }
        Value::Array(items) => items.iter_mut().for_each(|v| truncate_strings(v, max)),
        Value::Object(map) => map.values_mut().for_each(|v| truncate_strings(v, max)),
        _ => {}
    }
}

async fn drive(cfg: &Config, spec: &RunSpec, link: &mut Link) -> Result<()> {
    let mut report = Reporter { tx: link.tx.clone(), buffer: None, turn_text: String::new() };
    tokio::fs::create_dir_all(&cfg.workspace).await.context("could not create the workspace directory")?;
    let workspace = cfg.workspace.canonicalize()?;

    let git_env = workspace::git_env(spec, &cfg.token);
    for repo in &spec.repos {
        let note = workspace::checkout_repo(&workspace, repo, &spec.work_branch, &git_env)
            .await
            .with_context(|| format!("could not check out {}", repo.full_name))?;
        report.system(note);
    }
    if !spec.skills.is_empty() {
        workspace::write_skills(&workspace, &spec.skills).await?;
        let names: Vec<&str> = spec.skills.iter().map(|s| s.name.as_str()).collect();
        report.system(format!("Installed skills: {}", names.join(", ")));
    }

    let mut env = spec.harness.env.clone();
    env.extend(git_env);
    env.insert("HYPERBEAM_SESSION_ID".into(), spec.session_id.to_string());
    env.insert("HYPERBEAM_MCP_URL".into(), spec.mcp_url.clone());

    // `{runner}` lets a profile launch this binary's built-in agents.
    let exe = std::env::current_exe().context("cannot locate the runner binary")?;
    let command: Vec<String> =
        spec.harness.command.iter().map(|arg| arg.replace("{runner}", &exe.to_string_lossy())).collect();
    let acp::Spawned { client, mut incoming, mut child } = acp::spawn(&command, &env, &workspace)?;
    let mut stderr_tail: VecDeque<String> = VecDeque::new();

    // Handshake. Harness stderr is collected meanwhile so a failure to start
    // can be explained.
    let handshake = handshake(&client, spec, &cfg.token, &workspace);
    tokio::pin!(handshake);
    let deadline = tokio::time::sleep(HANDSHAKE_TIMEOUT);
    tokio::pin!(deadline);
    let (acp_session, agent) = loop {
        tokio::select! {
            done = &mut handshake => match done {
                Ok(ok) => break ok,
                Err(err) => return Err(explain(err, &stderr_tail)),
            },
            msg = incoming.recv() => match msg {
                Some(Incoming::Stderr(line)) => push_tail(&mut stderr_tail, line),
                Some(Incoming::Closed) | None => {
                    return Err(explain(anyhow!("the harness exited during startup"), &stderr_tail));
                }
                Some(Incoming::Request { id, method, .. }) => {
                    client.respond_error(id, -32601, &format!("`{method}` is not supported by this client"));
                }
                Some(Incoming::Notification { .. }) => {}
            },
            _ = &mut deadline => bail!("the harness did not complete its handshake within {HANDSHAKE_TIMEOUT:?}"),
        }
    };
    let _ = link.tx.send(RunnerMsg::Ready { agent });
    if spec.probe {
        // The control plane only wanted to know what this harness offers.
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
        return Ok(());
    }

    let (turn_tx, mut turn_rx) = mpsc::unbounded_channel::<(Uuid, Result<Value>)>();
    let mut queue: VecDeque<(Uuid, String)> = VecDeque::new();
    let mut active: Option<Uuid> = None;
    let mut first_prompt = true;
    let mut flush = tokio::time::interval(FLUSH_INTERVAL);

    loop {
        if active.is_none() {
            if let Some((input_id, text)) = queue.pop_front() {
                let text = if std::mem::take(&mut first_prompt) && !spec.preamble.is_empty() {
                    format!("{}\n\n---\n\n{}", spec.preamble, text)
                } else {
                    text
                };
                active = Some(input_id);
                report.turn_text.clear();
                let _ = link.tx.send(RunnerMsg::TurnStarted { input_id });
                let client = client.clone();
                let turn_tx = turn_tx.clone();
                let params = json!({ "sessionId": acp_session, "prompt": [{ "type": "text", "text": text }] });
                tokio::spawn(async move {
                    let result = client.request("session/prompt", params).await;
                    let _ = turn_tx.send((input_id, result));
                });
            }
        }

        tokio::select! {
            msg = link.rx.recv() => match msg {
                Some(ServerMsg::Prompt { input_id, text }) => queue.push_back((input_id, text)),
                Some(ServerMsg::Cancel) => {
                    queue.clear();
                    if active.is_some() {
                        client.notify("session/cancel", json!({ "sessionId": acp_session }));
                    }
                }
                Some(ServerMsg::Spec { .. }) => {}
                Some(ServerMsg::Shutdown) | None => break,
            },
            Some((input_id, result)) = turn_rx.recv() => {
                report.flush();
                active = None;
                let stop_reason = match result {
                    Ok(value) => value.get("stopReason").and_then(Value::as_str).unwrap_or("end_turn").to_string(),
                    Err(err) => {
                        report.event(event::ERROR, json!({ "text": format!("{err:#}") }));
                        "error".to_string()
                    }
                };
                let _ = link.tx.send(RunnerMsg::TurnEnded { input_id, stop_reason });
            }
            msg = incoming.recv() => match msg {
                Some(Incoming::Notification { method, params }) if method == "session/update" => {
                    on_update(&mut report, params);
                }
                Some(Incoming::Notification { .. }) => {}
                Some(Incoming::Request { id, method, params }) => {
                    if method == "session/request_permission" {
                        on_permission(&mut report, &client, id, &params);
                    } else {
                        client.respond_error(id, -32601, &format!("`{method}` is not supported by this client"));
                    }
                }
                Some(Incoming::Stderr(line)) => {
                    tracing::debug!(target: "harness", "{line}");
                    push_tail(&mut stderr_tail, line);
                }
                Some(Incoming::Closed) | None => {
                    report.flush();
                    return Err(explain(anyhow!("the harness exited unexpectedly"), &stderr_tail));
                }
            },
            _ = flush.tick() => report.flush(),
        }
    }

    report.flush();
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
    Ok(())
}

fn push_tail(tail: &mut VecDeque<String>, line: String) {
    if tail.len() >= 20 {
        tail.pop_front();
    }
    tail.push_back(line);
}

fn explain(err: anyhow::Error, stderr_tail: &VecDeque<String>) -> anyhow::Error {
    if stderr_tail.is_empty() {
        return err;
    }
    let tail: Vec<&str> = stderr_tail.iter().map(String::as_str).collect();
    anyhow!("{err:#}\nharness output:\n{}", tail.join("\n"))
}

async fn handshake(
    client: &AcpClient,
    spec: &RunSpec,
    token: &str,
    workspace: &std::path::Path,
) -> Result<(String, Value)> {
    let init = client
        .request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false },
                "clientInfo": { "name": "hyperbeam", "title": "Hyperbeam", "version": env!("CARGO_PKG_VERSION") },
            }),
        )
        .await?;

    let http_mcp = init
        .pointer("/agentCapabilities/mcpCapabilities/http")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mcp_servers = if spec.probe {
        // A probe has no session behind it, so no tool gateway to offer.
        json!([])
    } else if http_mcp {
        json!([{
            "type": "http",
            "name": "hyperbeam",
            "url": spec.mcp_url,
            "headers": [{ "name": "Authorization", "value": format!("Bearer {token}") }],
        }])
    } else {
        // Every ACP agent must support stdio servers; bridge to the gateway.
        let exe = std::env::current_exe().context("cannot locate the runner binary")?;
        json!([{
            "name": "hyperbeam",
            "command": exe,
            "args": ["mcp-bridge"],
            "env": [
                { "name": "HYPERBEAM_MCP_URL", "value": spec.mcp_url },
                { "name": "HYPERBEAM_RUN_TOKEN", "value": token },
            ],
        }])
    };

    let session = client
        .request("session/new", json!({ "cwd": workspace, "mcpServers": mcp_servers }))
        .await
        .context("the harness could not start a session (check its credential)")?;
    let session_id = session
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("the harness returned no session id"))?
        .to_string();

    // Model selection is a standard ACP session option (category `model`).
    let model_option = session
        .get("configOptions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|o| o["category"] == "model" || o["id"] == "model")
        .cloned();
    let models = model_option.as_ref().map(|o| flatten_options(&o["options"])).unwrap_or_default();
    let mut model = model_option.as_ref().and_then(|o| o["currentValue"].as_str()).map(str::to_string);
    let mut model_warning = None;
    if let Some(wanted) = spec.harness.model.as_deref().filter(|m| !m.is_empty()) {
        match &model_option {
            Some(option) if model.as_deref() == Some(wanted) => {
                let _ = option;
            }
            Some(option) => {
                let request = json!({ "sessionId": session_id, "configId": option["id"], "value": wanted });
                match client.request("session/set_config_option", request).await {
                    Ok(_) => model = Some(wanted.to_string()),
                    Err(err) => {
                        let known: Vec<&str> = models.iter().filter_map(|m| m["value"].as_str()).collect();
                        model_warning = Some(format!(
                            "Could not select model `{wanted}`; continuing on the harness default. It offers: {}. ({err:#})",
                            known.join(", ")
                        ));
                    }
                }
            }
            // No ACP option: the profile's model environment variable, if any, has already been set.
            None => model = Some(wanted.to_string()),
        }
    }

    let agent = json!({
        "info": init.get("agentInfo").cloned().unwrap_or(Value::Null),
        "protocolVersion": init.get("protocolVersion").cloned().unwrap_or(Value::Null),
        "mcpTransport": if http_mcp { "http" } else { "stdio-bridge" },
        "model": model,
        "models": models,
        "modelWarning": model_warning,
    });
    Ok((session_id, agent))
}

/// Select options may be flat or grouped; return them flat as `{value, name, description}`.
fn flatten_options(options: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for item in options.as_array().into_iter().flatten() {
        if let Some(group) = item.get("options").and_then(Value::as_array) {
            out.extend(group.iter().filter(|o| o["value"].is_string()).cloned());
        } else if item["value"].is_string() {
            out.push(item.clone());
        }
    }
    out.iter().map(|o| json!({ "value": o["value"], "name": o["name"], "description": o["description"] })).collect()
}

fn on_update(report: &mut Reporter, params: Value) {
    let Some(update) = params.get("update") else { return };
    let kind = update.get("sessionUpdate").and_then(Value::as_str).unwrap_or_default();
    let stripped = || {
        let mut data = update.clone();
        if let Some(map) = data.as_object_mut() {
            map.remove("sessionUpdate");
            map.remove("_meta");
        }
        data
    };
    match kind {
        "agent_message_chunk" | "agent_thought_chunk" => {
            let event_kind = if kind == "agent_message_chunk" { event::AGENT_MESSAGE } else { event::AGENT_THOUGHT };
            match update.pointer("/content/text").and_then(Value::as_str) {
                Some(text) => report.text(event_kind, text),
                None => report.event(event_kind, json!({ "content": update.get("content") })),
            }
        }
        "tool_call" => report.event(event::TOOL_CALL, stripped()),
        "tool_call_update" => report.event(event::TOOL_CALL_UPDATE, stripped()),
        "plan" => report.event(event::PLAN, json!({ "entries": update.get("entries") })),
        _ => {}
    }
}

/// The sandbox and the tool gateway are the security boundary, so in-harness
/// permission prompts are answered automatically and recorded.
fn on_permission(report: &mut Reporter, client: &AcpClient, id: Value, params: &Value) {
    let options = params.get("options").and_then(Value::as_array).cloned().unwrap_or_default();
    let pick = |kind: &str| options.iter().find(|o| o.get("kind").and_then(Value::as_str) == Some(kind));
    let chosen = pick("allow_always").or_else(|| pick("allow_once")).or_else(|| options.first());
    match chosen.and_then(|o| o.get("optionId")) {
        Some(option_id) => {
            client.respond(id, json!({ "outcome": { "outcome": "selected", "optionId": option_id } }));
            report.event(
                event::PERMISSION,
                json!({
                    "title": params.pointer("/toolCall/title"),
                    "toolCallId": params.pointer("/toolCall/toolCallId"),
                    "decision": chosen.and_then(|o| o.get("name")),
                }),
            );
        }
        None => client.respond(id, json!({ "outcome": { "outcome": "cancelled" } })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_on_char_boundaries() {
        let mut v = json!({ "a": ["é".repeat(10)], "b": "short" });
        truncate_strings(&mut v, 5);
        let s = v["a"][0].as_str().unwrap();
        assert!(s.starts_with("éé\n… ["));
        assert_eq!(v["b"], "short");
    }

    #[test]
    fn flattens_grouped_and_flat_model_options() {
        let flat = json!([{ "value": "a", "name": "A" }]);
        let grouped = json!([{ "group": "g", "name": "G", "options": [{ "value": "b", "name": "B", "description": "d" }] }]);
        assert_eq!(flatten_options(&flat)[0]["value"], "a");
        assert_eq!(flatten_options(&grouped), vec![json!({ "value": "b", "name": "B", "description": "d" })]);
        assert!(flatten_options(&Value::Null).is_empty());
    }

    #[test]
    fn reporter_coalesces_adjacent_text() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut r = Reporter { tx, buffer: None, turn_text: String::new() };
        r.text(event::AGENT_MESSAGE, "Hello ");
        r.text(event::AGENT_MESSAGE, "world");
        r.text(event::AGENT_THOUGHT, "hmm");
        r.event(event::PLAN, json!({ "entries": [] }));
        let mut seen = Vec::new();
        while let Ok(RunnerMsg::Event { kind, data }) = rx.try_recv() {
            seen.push((kind, data));
        }
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0].1["text"], "Hello world");
        assert_eq!(seen[1].0, event::AGENT_THOUGHT);
        assert_eq!(seen[2].0, event::PLAN);
    }
}
