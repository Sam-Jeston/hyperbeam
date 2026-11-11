//! stdio-to-HTTP bridge for the tool gateway, for harnesses that can only
//! launch MCP servers as local processes.

use anyhow::Result;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub async fn run(url: String, token: String) -> Result<()> {
    let http = reqwest::Client::new();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = request.get("id").cloned();
        let reply = match http.post(&url).bearer_auth(&token).json(&request).send().await {
            Ok(resp) if resp.status() == reqwest::StatusCode::ACCEPTED => None,
            Ok(resp) => match resp.json::<Value>().await {
                Ok(body) => Some(body),
                Err(err) => id.map(|id| error(id, &format!("gateway sent an unreadable reply: {err}"))),
            },
            Err(err) => id.map(|id| error(id, &format!("gateway unreachable: {err}"))),
        };
        if let Some(reply) = reply {
            stdout.write_all(reply.to_string().as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

fn error(id: Value, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": message } })
}
