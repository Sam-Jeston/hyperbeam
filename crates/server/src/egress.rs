//! Egress allowlist proxy.
//!
//! In Kubernetes, a NetworkPolicy lets agent pods talk only to the control
//! plane. Everything else they need (the model API, package registries) goes
//! through this HTTP CONNECT proxy, which authenticates the session and only
//! tunnels to hosts on the allowlist. It never sees inside the TLS stream.

use crate::app::App;
use crate::store;
use anyhow::Result;
use base64::Engine;
use hyperbeam_core::protocol::event;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Does `host` match an allowlist pattern (`example.com` or `*.example.com`)?
pub fn host_allowed(host: &str, patterns: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    patterns.iter().any(|pattern| {
        let pattern = pattern.trim().to_ascii_lowercase();
        match pattern.strip_prefix("*.") {
            Some(suffix) => host.len() > suffix.len() + 1 && host.ends_with(suffix) && host[..host.len() - suffix.len()].ends_with('.'),
            None => !pattern.is_empty() && host == pattern,
        }
    })
}

struct ConnectRequest {
    host: String,
    port: u16,
    token: Option<String>,
}

fn parse_request(head: &str) -> Result<ConnectRequest, &'static str> {
    let mut lines = head.split("\r\n");
    let mut request = lines.next().unwrap_or_default().split(' ');
    let (method, target) = (request.next().unwrap_or_default(), request.next().unwrap_or_default());
    if !method.eq_ignore_ascii_case("CONNECT") {
        return Err("only HTTPS (CONNECT) is proxied");
    }
    let (host, port) = target.rsplit_once(':').ok_or("malformed CONNECT target")?;
    let port: u16 = port.parse().map_err(|_| "malformed CONNECT port")?;
    let mut token = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("proxy-authorization") {
                if let Some(encoded) = value.trim().strip_prefix("Basic ") {
                    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded.trim()) {
                        token = String::from_utf8_lossy(&decoded).split_once(':').map(|(_, password)| password.to_string());
                    }
                }
            }
        }
    }
    Ok(ConnectRequest { host: host.trim_matches(['[', ']']).to_string(), port, token })
}

async fn reply(stream: &mut TcpStream, status: &str, message: &str) {
    let extra = if status.starts_with("407") { "Proxy-Authenticate: Basic realm=\"hyperbeam\"\r\n" } else { "" };
    let response = format!(
        "HTTP/1.1 {status}\r\n{extra}Content-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{message}",
        message.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

async fn serve(app: App, mut stream: TcpStream) -> Result<()> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() > 16 * 1024 {
            reply(&mut stream, "431 Request Header Fields Too Large", "hyperbeam: request too large\n").await;
            return Ok(());
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let request = match parse_request(&head) {
        Ok(request) => request,
        Err(why) => {
            reply(&mut stream, "405 Method Not Allowed", &format!("hyperbeam egress proxy: {why}\n")).await;
            return Ok(());
        }
    };
    let Some(token) = request.token else {
        reply(&mut stream, "407 Proxy Authentication Required", "hyperbeam egress proxy: session credential required\n").await;
        return Ok(());
    };
    // A model-discovery probe is not a session, but its harness may reach
    // the same hosts while it starts.
    let session = app.session_for_token(&token).await?;
    let harness_id = match &session {
        Some(session) => session.harness_id,
        None => match app.probes.by_token(&token) {
            Some(probe) => probe.harness_id,
            None => {
                reply(&mut stream, "407 Proxy Authentication Required", "hyperbeam egress proxy: unknown or ended session\n").await;
                return Ok(());
            }
        },
    };

    let mut allowed = app.cfg.egress_allow.clone();
    if let Some(harness) = store::harness_by_id(&app.db, harness_id).await? {
        allowed.extend(harness.egress_hosts.0);
    }
    if let Some(extra) = store::get_setting(&app.db, "egress_allow").await? {
        allowed.extend(extra.as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)));
    }
    if request.port != 443 || !host_allowed(&request.host, &allowed) {
        let target = format!("{}:{}", request.host, request.port);
        if let Some(session) = &session {
            store::record_audit(&app.db, session, "egress", &target, false, "host is not on the allowlist").await.ok();
            app.append_event(
                session.id,
                event::GATEWAY,
                json!({
                    "tool": "egress", "target": target, "allowed": false,
                    "summary": format!("network access denied: {target} is not on the egress allowlist"),
                }),
            )
            .await
            .ok();
        } else {
            tracing::info!("model discovery for harness {harness_id}: egress to {target} denied");
        }
        reply(&mut stream, "403 Forbidden", &format!("hyperbeam egress proxy: {target} is not on the allowlist\n")).await;
        return Ok(());
    }

    let mut upstream = match TcpStream::connect((request.host.as_str(), request.port)).await {
        Ok(upstream) => upstream,
        Err(err) => {
            reply(&mut stream, "502 Bad Gateway", &format!("hyperbeam egress proxy: {err}\n")).await;
            return Ok(());
        }
    };
    stream.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;
    if buf.len() > head_end {
        upstream.write_all(&buf[head_end..]).await?;
    }
    tokio::io::copy_bidirectional(&mut stream, &mut upstream).await.ok();
    Ok(())
}

pub async fn run(app: App, listener: TcpListener) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let app = app.clone();
                tokio::spawn(async move {
                    if let Err(err) = serve(app, stream).await {
                        tracing::debug!("egress connection ended: {err}");
                    }
                });
            }
            Err(err) => {
                tracing::warn!("egress accept failed: {err}");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_matching() {
        let patterns = vec!["api.anthropic.com".to_string(), "*.npmjs.org".to_string()];
        assert!(host_allowed("api.anthropic.com", &patterns));
        assert!(host_allowed("API.Anthropic.com.", &patterns));
        assert!(host_allowed("registry.npmjs.org", &patterns));
        assert!(!host_allowed("npmjs.org", &patterns));
        assert!(!host_allowed("evilnpmjs.org", &patterns));
        assert!(!host_allowed("api.anthropic.com.evil.io", &patterns));
        assert!(!host_allowed("anything", &[String::new()]));
    }

    #[test]
    fn parses_connect() {
        let auth = base64::engine::general_purpose::STANDARD.encode("session:nmr_abc");
        let head = format!("CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: x\r\nProxy-Authorization: Basic {auth}\r\n\r\n");
        let req = parse_request(&head).unwrap();
        assert_eq!((req.host.as_str(), req.port, req.token.as_deref()), ("api.anthropic.com", 443, Some("nmr_abc")));
        assert!(parse_request("GET http://x/ HTTP/1.1\r\n\r\n").is_err());
    }
}
