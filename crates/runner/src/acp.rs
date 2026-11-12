//! A minimal Agent Client Protocol client: JSON-RPC 2.0, one message per
//! line, over the harness process's stdio.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, mpsc, oneshot};

/// A message the agent initiated.
#[derive(Debug)]
pub enum Incoming {
    Notification { method: String, params: Value },
    Request { id: Value, method: String, params: Value },
    /// A line the harness wrote to stderr.
    Stderr(String),
    /// The harness's stdout closed: it exited or crashed.
    Closed,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, Value>>>>>;

#[derive(Clone)]
pub struct AcpClient {
    out: mpsc::UnboundedSender<String>,
    pending: Pending,
    next_id: Arc<AtomicU64>,
}

pub struct Spawned {
    pub client: AcpClient,
    pub incoming: mpsc::UnboundedReceiver<Incoming>,
    pub child: Child,
}

pub fn spawn(command: &[String], env: &BTreeMap<String, String>, cwd: &Path) -> Result<Spawned> {
    let (program, args) = command.split_first().ok_or_else(|| anyhow!("harness command is empty"))?;
    let mut child = Command::new(program)
        .args(args)
        .envs(env)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("could not start harness `{program}`"))?;

    let mut stdin = child.stdin.take().context("harness stdin unavailable")?;
    let stdout = child.stdout.take().context("harness stdout unavailable")?;
    let stderr = child.stderr.take().context("harness stderr unavailable")?;

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    let (in_tx, in_rx) = mpsc::unbounded_channel::<Incoming>();
    let pending: Pending = Arc::default();

    tokio::spawn(async move {
        while let Some(line) = out_rx.recv().await {
            if stdin.write_all(line.as_bytes()).await.is_err() || stdin.write_all(b"\n").await.is_err() {
                break;
            }
            let _ = stdin.flush().await;
        }
    });

    let err_tx = in_tx.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if err_tx.send(Incoming::Stderr(line)).is_err() {
                break;
            }
        }
    });

    let reader_pending = pending.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(msg) = serde_json::from_str::<Value>(line) else {
                // Some harnesses print banners on stdout before speaking ACP.
                let _ = in_tx.send(Incoming::Stderr(line.to_string()));
                continue;
            };
            let method = msg.get("method").and_then(Value::as_str).map(str::to_string);
            let id = msg.get("id").cloned().filter(|v| !v.is_null());
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let sent = match (method, id) {
                (Some(method), Some(id)) => in_tx.send(Incoming::Request { id, method, params }),
                (Some(method), None) => in_tx.send(Incoming::Notification { method, params }),
                (None, Some(id)) => {
                    if let Some(n) = id.as_u64()
                        && let Some(tx) = reader_pending.lock().await.remove(&n) {
                            let result = match msg.get("error") {
                                Some(err) if !err.is_null() => Err(err.clone()),
                                _ => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                            };
                            let _ = tx.send(result);
                        }
                    Ok(())
                }
                (None, None) => Ok(()),
            };
            if sent.is_err() {
                break;
            }
        }
        // Fail anything still waiting so callers do not hang forever.
        reader_pending.lock().await.clear();
        let _ = in_tx.send(Incoming::Closed);
    });

    Ok(Spawned {
        client: AcpClient { out: out_tx, pending, next_id: Arc::new(AtomicU64::new(1)) },
        incoming: in_rx,
        child,
    })
}

impl AcpClient {
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        if self.out.send(line).is_err() {
            bail!("harness is not running");
        }
        match rx.await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(err)) => {
                let message = err.get("message").and_then(Value::as_str).unwrap_or("unknown error");
                let detail = err.get("data").map(|d| format!(" ({d})")).unwrap_or_default();
                bail!("harness rejected `{method}`: {message}{detail}")
            }
            Err(_) => bail!("harness exited before answering `{method}`"),
        }
    }

    pub fn notify(&self, method: &str, params: Value) {
        let _ = self.out.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }).to_string());
    }

    pub fn respond(&self, id: Value, result: Value) {
        let _ = self.out.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string());
    }

    pub fn respond_error(&self, id: Value, code: i64, message: &str) {
        let _ = self.out.send(
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string(),
        );
    }
}
