//! A scriptable ACP agent with no model behind it, for the end-to-end tests
//! in `crates/server/tests`. It is not shipped. Lines in the prompt that begin
//! with `!` are directives:
//!
//! ```text
//! !say <text>           stream a message
//! !think <text>         stream a thought
//! !plan a; b; c         publish a plan
//! !sh <command>         run a shell command in the workspace
//! !mcp <tool> <json>    call a tool on the Hyperbeam gateway
//! !model               say which model is selected
//! !sleep <seconds>
//! !fail <message>       end the turn with an error
//! ```

use anyhow::Result;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, mpsc};

#[derive(Default)]
struct State {
    cwd: PathBuf,
    mcp_url: Option<String>,
    mcp_auth: Option<String>,
    model: String,
}

struct Agent {
    out: mpsc::UnboundedSender<Value>,
    state: Mutex<State>,
    cancelled: AtomicBool,
    next_tool: AtomicU64,
    http: reqwest::Client,
}

#[tokio::main]
async fn main() -> Result<()> {
    let (out, mut out_rx) = mpsc::unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(msg) = out_rx.recv().await {
            let _ = stdout.write_all(format!("{msg}\n").as_bytes()).await;
            let _ = stdout.flush().await;
        }
    });

    let agent = Arc::new(Agent {
        out,
        state: Mutex::default(),
        cancelled: AtomicBool::new(false),
        next_tool: AtomicU64::new(1),
        http: reqwest::Client::new(),
    });

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or_default().to_string();
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        match method.as_str() {
            "initialize" => agent.reply(
                id,
                json!({
                    "protocolVersion": 1,
                    "agentCapabilities": {
                        "loadSession": false,
                        "mcpCapabilities": { "http": true, "sse": false },
                        "promptCapabilities": {},
                    },
                    "agentInfo": { "name": "hyperbeam-test-agent", "title": "Test agent", "version": env!("CARGO_PKG_VERSION") },
                    "authMethods": [],
                }),
            ),
            "session/new" => {
                let mut state = agent.state.lock().await;
                state.cwd = params.get("cwd").and_then(Value::as_str).map(PathBuf::from).unwrap_or_default();
                for server in params.get("mcpServers").and_then(Value::as_array).into_iter().flatten() {
                    if server.get("type").and_then(Value::as_str) == Some("http") {
                        state.mcp_url = server.get("url").and_then(Value::as_str).map(str::to_string);
                        state.mcp_auth = server
                            .get("headers")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .find(|h| h["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case("authorization")))
                            .and_then(|h| h["value"].as_str().map(str::to_string));
                    }
                }
                state.model = "mock-small".to_string();
                drop(state);
                agent.reply(
                    id,
                    json!({
                        "sessionId": "mock-session",
                        "configOptions": [{
                            "id": "model", "name": "Model", "category": "model", "type": "select",
                            "currentValue": "mock-small",
                            "options": [
                                { "value": "mock-small", "name": "Mock Small", "description": "Quick and cheap" },
                                { "value": "mock-large", "name": "Mock Large", "description": "Slow and thorough" },
                            ],
                        }],
                    }),
                );
            }
            "session/set_config_option" => {
                let value = params.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
                if params["configId"] == "model" && ["mock-small", "mock-large"].contains(&value.as_str()) {
                    agent.state.lock().await.model = value;
                    agent.reply(id, json!({ "configOptions": [] }));
                } else if let Some(id) = id {
                    agent.send(json!({
                        "jsonrpc": "2.0", "id": id, "error": { "code": -32602, "message": format!("unknown model `{value}`") },
                    }));
                }
            }
            "session/prompt" => {
                let agent = agent.clone();
                tokio::spawn(async move {
                    agent.cancelled.store(false, Ordering::SeqCst);
                    let text: String = params
                        .get("prompt")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|block| block.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n");
                    match agent.turn(&text).await {
                        Ok(()) => {
                            let reason = if agent.cancelled.load(Ordering::SeqCst) { "cancelled" } else { "end_turn" };
                            agent.reply(id, json!({ "stopReason": reason }));
                        }
                        Err(message) => agent.send(json!({
                            "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": message },
                        })),
                    }
                });
            }
            "session/cancel" => agent.cancelled.store(true, Ordering::SeqCst),
            _ => {
                if let Some(id) = id {
                    agent.send(json!({
                        "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "method not found" },
                    }));
                }
            }
        }
    }
    drop(agent);
    let _ = writer.await;
    Ok(())
}

impl Agent {
    fn send(&self, msg: Value) {
        let _ = self.out.send(msg);
    }

    fn reply(&self, id: Option<Value>, result: Value) {
        if let Some(id) = id {
            self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
        }
    }

    fn update(&self, update: Value) {
        self.send(json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": { "sessionId": "mock-session", "update": update },
        }));
    }

    async fn stream(&self, kind: &str, text: &str) {
        for word in text.split_inclusive(' ') {
            self.update(json!({ "sessionUpdate": kind, "content": { "type": "text", "text": word } }));
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    async fn turn(&self, prompt: &str) -> std::result::Result<(), String> {
        // Ignore the platform's preamble: only the request itself is a script.
        let prompt = prompt.rsplit("\n\n---\n\n").next().unwrap_or(prompt);
        let directives: Vec<&str> = prompt.lines().map(str::trim).filter(|l| l.starts_with('!')).collect();
        if directives.is_empty() {
            self.update(json!({
                "sessionUpdate": "plan",
                "entries": [
                    { "content": "Read the request", "priority": "high", "status": "completed" },
                    { "content": "Reply", "priority": "medium", "status": "in_progress" },
                ],
            }));
            let request = prompt.trim();
            let short: String = request.chars().take(200).collect();
            self.stream(
                "agent_message_chunk",
                &format!("This is the test agent; no model is attached. I received: \"{short}\""),
            )
            .await;
            return Ok(());
        }

        for line in directives {
            if self.cancelled() {
                break;
            }
            let (command, rest) = line[1..].split_once(' ').unwrap_or((&line[1..], ""));
            let rest = rest.trim();
            match command {
                "say" => self.stream("agent_message_chunk", rest).await,
                "think" => self.stream("agent_thought_chunk", rest).await,
                "plan" => {
                    let entries: Vec<Value> = rest
                        .split(';')
                        .map(|s| json!({ "content": s.trim(), "priority": "medium", "status": "pending" }))
                        .collect();
                    self.update(json!({ "sessionUpdate": "plan", "entries": entries }));
                }
                "sleep" => {
                    let total = Duration::from_secs_f64(rest.parse().unwrap_or(1.0));
                    let started = tokio::time::Instant::now();
                    while started.elapsed() < total && !self.cancelled() {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
                "model" => {
                    let model = self.state.lock().await.model.clone();
                    self.stream("agent_message_chunk", &format!("model={model} ")).await;
                }
                "sh" => self.shell(rest).await,
                "mcp" => self.mcp(rest).await,
                "fail" => return Err(if rest.is_empty() { "mock failure".to_string() } else { rest.to_string() }),
                other => self.stream("agent_message_chunk", &format!("(unknown directive !{other}) ")).await,
            }
        }
        Ok(())
    }

    fn begin_tool(&self, title: &str, kind: &str, raw_input: Value) -> String {
        let id = format!("mock-tool-{}", self.next_tool.fetch_add(1, Ordering::SeqCst));
        self.update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": id,
            "title": title,
            "kind": kind,
            "status": "in_progress",
            "rawInput": raw_input,
        }));
        id
    }

    fn end_tool(&self, id: &str, ok: bool, output: &str) {
        self.update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": id,
            "status": if ok { "completed" } else { "failed" },
            "content": [{ "type": "content", "content": { "type": "text", "text": output } }],
        }));
    }

    async fn shell(&self, command: &str) {
        let cwd = self.state.lock().await.cwd.clone();
        let id = self.begin_tool(command, "execute", json!({ "command": command }));
        let result = tokio::process::Command::new("sh").arg("-c").arg(command).current_dir(cwd).output().await;
        match result {
            Ok(out) => {
                let mut text = String::from_utf8_lossy(&out.stdout).to_string();
                text.push_str(&String::from_utf8_lossy(&out.stderr));
                self.end_tool(&id, out.status.success(), text.trim_end());
            }
            Err(err) => self.end_tool(&id, false, &err.to_string()),
        }
    }

    async fn mcp(&self, rest: &str) {
        let (tool, args) = rest.split_once(' ').unwrap_or((rest, "{}"));
        let arguments: Value = serde_json::from_str(args).unwrap_or_else(|_| json!({}));
        let id = self.begin_tool(&format!("hyperbeam · {tool}"), "other", arguments.clone());
        let (url, auth) = {
            let state = self.state.lock().await;
            (state.mcp_url.clone(), state.mcp_auth.clone())
        };
        let Some(url) = url else {
            self.end_tool(&id, false, "no tool gateway configured");
            return;
        };
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": tool, "arguments": arguments },
        });
        let mut request = self.http.post(url).json(&body);
        if let Some(auth) = auth {
            request = request.header("authorization", auth);
        }
        match request.send().await {
            Ok(resp) => match resp.json::<Value>().await {
                Ok(reply) => {
                    if let Some(err) = reply.get("error") {
                        self.end_tool(&id, false, &err.to_string());
                        return;
                    }
                    let is_error = reply.pointer("/result/isError").and_then(Value::as_bool).unwrap_or(false);
                    let text: String = reply
                        .pointer("/result/content")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|c| c.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.end_tool(&id, !is_error, &text);
                }
                Err(err) => self.end_tool(&id, false, &err.to_string()),
            },
            Err(err) => self.end_tool(&id, false, &err.to_string()),
        }
    }
}
