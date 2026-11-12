//! A small MCP client (streamable HTTP) for talking to upstream servers on an
//! agent's behalf. Replies may be plain JSON or a server-sent event stream.

use anyhow::{Context, Result, anyhow, bail};
use futures::StreamExt;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_TYPE, HeaderMap};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

const PROTOCOL_VERSION: &str = "2025-06-18";
/// Largest reply accepted from an upstream server.
const MAX_REPLY: usize = 8 * 1024 * 1024;

/// One upstream server, with the headers (credential included) to send it.
#[derive(Clone)]
pub struct Endpoint {
    /// For messages: the connection's title.
    pub label: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl Endpoint {
    fn cache_key(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(self.url.as_bytes());
        for (name, value) in &self.headers {
            hash.update([0]);
            hash.update(name.to_ascii_lowercase().as_bytes());
            hash.update([0]);
            hash.update(value.as_bytes());
        }
        hex::encode(hash.finalize())
    }
}

#[derive(Clone)]
struct UpstreamSession {
    id: Option<String>,
    version: String,
}

#[derive(Default)]
pub struct McpClient {
    /// Initialised sessions, by endpoint. Shared by every agent using the
    /// connection, since they all use the same credential.
    sessions: Mutex<HashMap<String, UpstreamSession>>,
}

impl McpClient {
    /// Forget every upstream session, e.g. after a connection changes.
    pub fn reset(&self) {
        self.sessions.lock().unwrap().clear();
    }

    pub async fn list_tools(&self, http: &reqwest::Client, endpoint: &Endpoint) -> Result<Vec<Value>> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..50 {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let result = self.request(http, endpoint, "tools/list", params, Duration::from_secs(30)).await?;
            tools.extend(result.get("tools").and_then(Value::as_array).cloned().unwrap_or_default());
            cursor = result.get("nextCursor").and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_string);
            if cursor.is_none() {
                return Ok(tools);
            }
        }
        bail!("{} listed more pages of tools than Hyperbeam will read", endpoint.label)
    }

    pub async fn call_tool(&self, http: &reqwest::Client, endpoint: &Endpoint, name: &str, arguments: &Value) -> Result<Value> {
        self.request(http, endpoint, "tools/call", json!({ "name": name, "arguments": arguments }), Duration::from_secs(120)).await
    }

    async fn request(&self, http: &reqwest::Client, endpoint: &Endpoint, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let key = endpoint.cache_key();
        for attempt in 0..2 {
            let session = self.sessions.lock().unwrap().get(&key).cloned();
            let session = match session {
                Some(s) => s,
                None => {
                    let s = self.initialize(http, endpoint).await?;
                    self.sessions.lock().unwrap().insert(key.clone(), s.clone());
                    s
                }
            };
            let body = json!({ "jsonrpc": "2.0", "id": 2, "method": method, "params": params });
            let (status, _, reply) = post(http, endpoint, Some(&session), &body, timeout).await?;
            // The server forgot the session: start a new one, once.
            if status == StatusCode::NOT_FOUND && session.id.is_some() && attempt == 0 {
                self.sessions.lock().unwrap().remove(&key);
                continue;
            }
            check_status(endpoint, status, reply.as_ref())?;
            return rpc_result(endpoint, reply);
        }
        unreachable!("the loop returns on its second attempt")
    }

    async fn initialize(&self, http: &reqwest::Client, endpoint: &Endpoint) -> Result<UpstreamSession> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "hyperbeam", "title": "Hyperbeam", "version": env!("CARGO_PKG_VERSION") },
            },
        });
        let (status, headers, reply) = post(http, endpoint, None, &body, Duration::from_secs(30)).await?;
        check_status(endpoint, status, reply.as_ref())?;
        let result = rpc_result(endpoint, reply)?;
        let session = UpstreamSession {
            id: headers.get("mcp-session-id").and_then(|v| v.to_str().ok()).map(str::to_string),
            version: result.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION).to_string(),
        };
        let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        post(http, endpoint, Some(&session), &note, Duration::from_secs(30)).await?;
        Ok(session)
    }
}

/// The server turned the credential down.
#[derive(Debug, thiserror::Error)]
#[error("{label} refused the credential ({status}). An admin can update it under Settings → Integrations.")]
pub struct Refused {
    pub label: String,
    pub status: StatusCode,
}

fn check_status(endpoint: &Endpoint, status: StatusCode, reply: Option<&Value>) -> Result<()> {
    if status.is_success() {
        return Ok(());
    }
    let label = &endpoint.label;
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(Refused { label: label.clone(), status }.into()),
        _ => {
            let message = reply.and_then(|r| r.pointer("/error/message")).and_then(Value::as_str).unwrap_or("");
            bail!("{label} answered {status}{}", if message.is_empty() { String::new() } else { format!(": {message}") })
        }
    }
}

fn rpc_result(endpoint: &Endpoint, reply: Option<Value>) -> Result<Value> {
    let reply = reply.ok_or_else(|| anyhow!("{} sent no reply", endpoint.label))?;
    if let Some(error) = reply.get("error") {
        bail!("{}: {}", endpoint.label, error.get("message").and_then(Value::as_str).unwrap_or("unknown error"));
    }
    reply.get("result").cloned().ok_or_else(|| anyhow!("{} sent a reply without a result", endpoint.label))
}

/// POST one JSON-RPC message. For a request, returns its response, read from
/// either a JSON body or an event stream; for a notification, nothing.
async fn post(
    http: &reqwest::Client,
    endpoint: &Endpoint,
    session: Option<&UpstreamSession>,
    body: &Value,
    timeout: Duration,
) -> Result<(StatusCode, HeaderMap, Option<Value>)> {
    let mut request = http
        .post(&endpoint.url)
        .header("accept", "application/json, text/event-stream")
        .json(body)
        .timeout(timeout);
    for (name, value) in &endpoint.headers {
        request = request.header(name, value);
    }
    if let Some(session) = session {
        request = request.header("mcp-protocol-version", &session.version);
        if let Some(id) = &session.id {
            request = request.header("mcp-session-id", id);
        }
    }
    let response = request.send().await.with_context(|| format!("could not reach {}", endpoint.label))?;
    let status = response.status();
    let headers = response.headers().clone();
    let Some(id) = body.get("id").cloned() else { return Ok((status, headers, None)) };
    if status == StatusCode::ACCEPTED || status == StatusCode::NO_CONTENT {
        return Ok((status, headers, None));
    }
    let content_type = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    let reply = if content_type.starts_with("text/event-stream") {
        read_event_stream(endpoint, response, &id).await?
    } else {
        let bytes = response.bytes().await.with_context(|| format!("{} sent an unreadable reply", endpoint.label))?;
        if bytes.len() > MAX_REPLY {
            bail!("{} sent a reply larger than {} MB", endpoint.label, MAX_REPLY / 1024 / 1024);
        }
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => Some(value),
            Err(_) if !status.is_success() => None,
            Err(_) => bail!("{} sent a reply that is not JSON", endpoint.label),
        }
    };
    Ok((status, headers, reply))
}

/// Read events until the response to request `id` arrives. Notifications and
/// requests from the server along the way are ignored.
async fn read_event_stream(endpoint: &Endpoint, response: reqwest::Response, id: &Value) -> Result<Option<Value>> {
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut read = 0usize;
    let mut data = String::new();
    loop {
        // Handle every complete line in the buffer.
        while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                if let Some(message) = take_message(&mut data, id) {
                    return Ok(Some(message));
                }
            } else if let Some(rest) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            }
        }
        match stream.next().await {
            Some(chunk) => {
                let chunk = chunk.with_context(|| format!("{} broke off its reply", endpoint.label))?;
                read += chunk.len();
                if read > MAX_REPLY {
                    bail!("{} sent a reply larger than {} MB", endpoint.label, MAX_REPLY / 1024 / 1024);
                }
                buffer.extend_from_slice(&chunk);
            }
            None => {
                // The stream ended; a last event may lack its blank line.
                return Ok(take_message(&mut data, id));
            }
        }
    }
}

fn take_message(data: &mut String, id: &Value) -> Option<Value> {
    let event = std::mem::take(data);
    let message: Value = serde_json::from_str(&event).ok()?;
    let is_reply = |m: &Value| m.get("id") == Some(id) && (m.get("result").is_some() || m.get("error").is_some());
    match message {
        Value::Array(batch) => batch.into_iter().find(is_reply),
        m if is_reply(&m) => Some(m),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_reply_out_of_an_event() {
        let id = json!(2);
        let mut data = r#"{"jsonrpc":"2.0","method":"notifications/progress","params":{}}"#.to_string();
        assert_eq!(take_message(&mut data, &id), None);
        let mut data = r#"{"jsonrpc":"2.0","id":2,"result":{"ok":true}}"#.to_string();
        assert_eq!(take_message(&mut data, &id).unwrap()["result"]["ok"], true);
        assert!(data.is_empty());
        // A request from the server reusing the id is not our reply.
        let mut data = r#"{"jsonrpc":"2.0","id":2,"method":"sampling/createMessage"}"#.to_string();
        assert_eq!(take_message(&mut data, &id), None);
    }
}
