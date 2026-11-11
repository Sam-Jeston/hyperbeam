//! The runner's link to the control plane: one outbound WebSocket that is
//! re-established if it drops, so a server restart does not kill a session.

use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use hyperbeam_core::protocol::{PROTOCOL_VERSION, RunnerMsg, ServerMsg};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// How long to keep retrying before giving the session up for dead.
const RECONNECT_WINDOW: Duration = Duration::from_secs(180);

pub struct Link {
    pub tx: mpsc::UnboundedSender<RunnerMsg>,
    pub rx: mpsc::UnboundedReceiver<ServerMsg>,
    task: tokio::task::JoinHandle<()>,
}

impl Link {
    /// Send whatever is still queued, then close the socket.
    pub async fn close(self) {
        let Link { tx, rx, task } = self;
        drop(tx);
        drop(rx);
        let _ = tokio::time::timeout(Duration::from_secs(5), task).await;
    }
}

pub fn ws_url(server_url: &str) -> Result<String> {
    let mut url = url::Url::parse(server_url).context("HYPERBEAM_SERVER_URL is not a valid URL")?;
    let scheme = match url.scheme() {
        "http" | "ws" => "ws",
        "https" | "wss" => "wss",
        other => anyhow::bail!("unsupported server URL scheme `{other}`"),
    };
    url.set_scheme(scheme).ok();
    let path = format!("{}/runner/v1/connect", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url.to_string())
}

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn dial(url: &str, token: &str) -> Result<Socket> {
    let mut request = url.into_client_request()?;
    request.headers_mut().insert("authorization", format!("Bearer {token}").parse()?);
    let (socket, _) = tokio_tungstenite::connect_async(request).await?;
    Ok(socket)
}

/// Connect and keep connected. Returns once the first connection is up.
pub async fn connect(server_url: &str, token: &str) -> Result<Link> {
    let url = ws_url(server_url)?;
    let mut socket = dial(&url, token).await.with_context(|| format!("could not reach {url}"))?;

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<RunnerMsg>();
    let (in_tx, in_rx) = mpsc::unbounded_channel::<ServerMsg>();
    let token = token.to_string();

    let task = tokio::spawn(async move {
        let mut resumed = false;
        // A message taken off the queue but not yet confirmed written.
        let mut unsent: Option<RunnerMsg> = None;
        'connection: loop {
            let hello = RunnerMsg::Hello {
                protocol: PROTOCOL_VERSION,
                runner_version: env!("CARGO_PKG_VERSION").to_string(),
                resumed,
            };
            let hello_ok = socket.send(Message::text(serde_json::to_string(&hello).unwrap())).await.is_ok();
            if hello_ok {
                let mut ping = tokio::time::interval(Duration::from_secs(20));
                loop {
                    if let Some(msg) = unsent.take() {
                        let text = serde_json::to_string(&msg).unwrap();
                        if socket.send(Message::text(text)).await.is_err() {
                            unsent = Some(msg);
                            break;
                        }
                        continue;
                    }
                    tokio::select! {
                        outgoing = out_rx.recv() => match outgoing {
                            Some(msg) => unsent = Some(msg),
                            None => {
                                let _ = socket.close(None).await;
                                return;
                            }
                        },
                        incoming = socket.next() => match incoming {
                            Some(Ok(Message::Text(text))) => {
                                match serde_json::from_str::<ServerMsg>(&text) {
                                    Ok(msg) => {
                                        // The receiver goes away during close(); keep
                                        // draining the outgoing queue regardless.
                                        let _ = in_tx.send(msg);
                                    }
                                    Err(err) => tracing::warn!("ignoring unreadable server message: {err}"),
                                }
                            }
                            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                            Some(Ok(_)) => {}
                        },
                        _ = ping.tick() => {
                            if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }

            tracing::warn!("lost connection to the control plane; reconnecting");
            resumed = true;
            let deadline = tokio::time::Instant::now() + RECONNECT_WINDOW;
            let mut delay = Duration::from_millis(500);
            loop {
                tokio::time::sleep(delay).await;
                match dial(&url, &token).await {
                    Ok(s) => {
                        socket = s;
                        continue 'connection;
                    }
                    Err(err) if tokio::time::Instant::now() >= deadline => {
                        tracing::error!("control plane unreachable, giving up: {err}");
                        // Dropping in_tx tells the session loop to shut down.
                        return;
                    }
                    Err(_) => delay = (delay * 2).min(Duration::from_secs(10)),
                }
            }
        }
    });

    Ok(Link { tx: out_tx, rx: in_rx, task })
}

#[cfg(test)]
mod tests {
    use super::ws_url;

    #[test]
    fn builds_ws_urls() {
        assert_eq!(ws_url("http://hyperbeam:8080").unwrap(), "ws://hyperbeam:8080/runner/v1/connect");
        assert_eq!(ws_url("https://n.example.com/base/").unwrap(), "wss://n.example.com/base/runner/v1/connect");
        assert!(ws_url("ftp://x").is_err());
    }
}
