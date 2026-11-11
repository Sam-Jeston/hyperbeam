//! Git smart-HTTP proxy.
//!
//! Sandboxes clone and push through here. The proxy authenticates the
//! session by its run token, checks the repository and the operation against
//! the session's grants, and only then forwards to GitHub with a credential
//! the sandbox never sees.

use crate::app::App;
use crate::auth::bearer;
use crate::integrations::github::Access;
use crate::store::{self, Session};
use axum::body::Body;
use axum::extract::{Path, RawQuery, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use hyperbeam_core::Grant;
use hyperbeam_core::protocol::event;
use serde_json::json;

const MAX_COMMAND_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefUpdate {
    pub old: String,
    pub new: String,
    pub name: String,
}

impl RefUpdate {
    pub fn is_delete(&self) -> bool {
        self.new.bytes().all(|b| b == b'0')
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    NeedMore,
    /// Ref updates, and the offset at which the pack data begins.
    Done(Vec<RefUpdate>, usize),
    Invalid(&'static str),
}

/// Parse the command list at the start of a `git-receive-pack` request body:
/// pkt-lines of `<old> <new> <ref>[\0capabilities]`, ended by a flush packet.
pub fn parse_push_commands(buf: &[u8]) -> Parsed {
    let mut pos = 0;
    let mut updates = Vec::new();
    loop {
        if buf.len() < pos + 4 {
            return Parsed::NeedMore;
        }
        let Some(len) = std::str::from_utf8(&buf[pos..pos + 4]).ok().and_then(|s| usize::from_str_radix(s, 16).ok()) else {
            return Parsed::Invalid("malformed packet length");
        };
        if len == 0 {
            return Parsed::Done(updates, pos + 4);
        }
        if len < 4 {
            return Parsed::Invalid("malformed packet length");
        }
        if buf.len() < pos + len {
            return Parsed::NeedMore;
        }
        let line = &buf[pos + 4..pos + len];
        pos += len;
        let line = line.split(|b| *b == 0).next().unwrap_or_default();
        let Ok(line) = std::str::from_utf8(line) else { return Parsed::Invalid("non UTF-8 ref name") };
        let line = line.trim_end_matches('\n');
        if line.starts_with("shallow ") {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(old), Some(new), Some(name)) if old.len() >= 40 && new.len() >= 40 => {
                updates.push(RefUpdate { old: old.to_string(), new: new.to_string(), name: name.to_string() });
            }
            _ => return Parsed::Invalid("unrecognised push command"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Fetch,
    Push,
}

fn deny(status: StatusCode, message: impl Into<String>) -> Response {
    (status, [(header::CONTENT_TYPE, "text/plain")], message.into()).into_response()
}

/// Split `owner/repo(.git)/rest` and validate the repository name.
fn split_path(path: &str) -> Option<(String, &str)> {
    let mut parts = path.splitn(3, '/');
    let owner = parts.next()?;
    let name = parts.next()?;
    let rest = parts.next()?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    let repo = format!("{owner}/{name}");
    hyperbeam_core::workflow::valid_repo(&repo).then_some((repo, rest))
}

async fn record(app: &App, session: &Session, action: &str, target: &str, allowed: bool, detail: &str) {
    store::record_audit(&app.db, session, action, target, allowed, detail).await.ok();
    let verb = if allowed { action.to_string() } else { format!("{action} denied") };
    let summary = if detail.is_empty() { format!("{verb}: {target}") } else { format!("{verb}: {target} ({detail})") };
    app.append_event(
        session.id,
        event::GATEWAY,
        json!({ "tool": action, "target": target, "allowed": allowed, "summary": summary, "detail": detail }),
    )
    .await
    .ok();
}

pub async fn handle(
    State(app): State<App>,
    Path(path): Path<String>,
    RawQuery(query): RawQuery,
    method: Method,
    headers: HeaderMap,
    parts: Parts,
    body: Body,
) -> Response {
    let Some(token) = bearer(&parts) else {
        return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Basic realm=\"hyperbeam\"")]).into_response();
    };
    let session = match app.session_for_token(&token).await {
        Ok(Some(session)) => session,
        Ok(None) => return deny(StatusCode::UNAUTHORIZED, "this session has ended; its git access is revoked\n"),
        Err(_) => return deny(StatusCode::INTERNAL_SERVER_ERROR, "hyperbeam: internal error\n"),
    };
    let Some((repo, rest)) = split_path(&path) else { return deny(StatusCode::NOT_FOUND, "not a repository path\n") };

    let query = query.unwrap_or_default();
    let op = match (method.as_str(), rest) {
        ("GET", "info/refs") if query.contains("service=git-upload-pack") => Op::Fetch,
        ("GET", "info/refs") if query.contains("service=git-receive-pack") => Op::Push,
        ("POST", "git-upload-pack") => Op::Fetch,
        ("POST", "git-receive-pack") => Op::Push,
        _ => return deny(StatusCode::NOT_FOUND, "hyperbeam proxies the git smart HTTP protocol only\n"),
    };
    let action = if op == Op::Push { "git push" } else { "git fetch" };

    let Some(registered) = session.repos.0.iter().find(|r| r.eq_ignore_ascii_case(&repo)).cloned() else {
        record(&app, &session, action, &repo, false, "repository is not part of this session").await;
        return deny(StatusCode::FORBIDDEN, format!("hyperbeam: {repo} is not one of this session's repositories\n"));
    };
    let needed = if op == Op::Push { Grant::GithubWrite } else { Grant::GithubRead };
    if !session.grants.has(needed) {
        record(&app, &session, action, &registered, false, &format!("requires {needed}")).await;
        return deny(StatusCode::FORBIDDEN, format!("hyperbeam: this session lacks {needed}\n"));
    }

    // For the push itself, read the ref updates before forwarding anything.
    let mut body_stream = body.into_data_stream();
    let mut prefix = BytesMut::new();
    let is_push_post = op == Op::Push && method == Method::POST;
    if is_push_post {
        if headers.contains_key(header::CONTENT_ENCODING) {
            return deny(StatusCode::BAD_REQUEST, "hyperbeam: compressed push requests are not accepted\n");
        }
        let updates = loop {
            match parse_push_commands(&prefix) {
                Parsed::Done(updates, _) => break updates,
                Parsed::Invalid(why) => return deny(StatusCode::BAD_REQUEST, format!("hyperbeam: {why}\n")),
                Parsed::NeedMore if prefix.len() > MAX_COMMAND_BYTES => {
                    return deny(StatusCode::BAD_REQUEST, "hyperbeam: push command list too large\n");
                }
                Parsed::NeedMore => match body_stream.next().await {
                    Some(Ok(chunk)) => prefix.extend_from_slice(&chunk),
                    _ => return deny(StatusCode::BAD_REQUEST, "hyperbeam: truncated push request\n"),
                },
            }
        };
        let default_branch = match store::repo_by_name(&app.db, &registered).await {
            Ok(Some(r)) => r.default_branch,
            _ => "main".to_string(),
        };
        let protected = format!("refs/heads/{default_branch}");
        let refs: Vec<&str> = updates.iter().map(|u| u.name.as_str()).collect();
        if updates.iter().any(|u| u.name == protected) && !session.grants.has(Grant::GithubMergePr) {
            let detail = format!("pushing to {default_branch} requires {}", Grant::GithubMergePr);
            record(&app, &session, action, &registered, false, &detail).await;
            return deny(
                StatusCode::FORBIDDEN,
                format!("hyperbeam: {detail}. Push your work branch and open a pull request instead.\n"),
            );
        }
        record(&app, &session, action, &registered, true, &refs.join(", ")).await;
    } else if op == Op::Fetch && method == Method::GET {
        store::record_audit(&app.db, &session, action, &registered, true, "").await.ok();
    }

    let access = if op == Op::Push { Access::Write } else { Access::Read };
    let upstream_token = match app.github_token(&registered, access).await {
        Ok(token) => token,
        Err(err) => return deny(StatusCode::BAD_GATEWAY, format!("hyperbeam: {err:#}\n")),
    };
    let mut url = format!("{}/{registered}.git/{rest}", app.cfg.github_git_url.trim_end_matches('/'));
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query);
    }
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{upstream_token}"));
    let mut request = app.http.request(method.clone(), url).header(header::AUTHORIZATION, format!("Basic {basic}"));
    for name in [header::CONTENT_TYPE, header::ACCEPT, header::CONTENT_ENCODING, header::CONTENT_LENGTH, header::USER_AGENT] {
        if let Some(value) = headers.get(&name) {
            request = request.header(name, value);
        }
    }
    if let Some(value) = headers.get("git-protocol") {
        request = request.header("git-protocol", value);
    }
    if method == Method::POST {
        let head = futures::stream::once(async move { Ok::<Bytes, axum::Error>(prefix.freeze()) });
        request = request.body(reqwest::Body::wrap_stream(head.chain(body_stream)));
    }

    let upstream = match request.send().await {
        Ok(response) => response,
        Err(err) => return deny(StatusCode::BAD_GATEWAY, format!("hyperbeam: could not reach GitHub: {err}\n")),
    };
    let mut response = Response::builder().status(upstream.status().as_u16());
    for name in [header::CONTENT_TYPE, header::CACHE_CONTROL, header::CONTENT_ENCODING] {
        if let Some(value) = upstream.headers().get(&name) {
            response = response.header(name, value);
        }
    }
    response
        .body(Body::from_stream(upstream.bytes_stream()))
        .unwrap_or_else(|_| deny(StatusCode::BAD_GATEWAY, "hyperbeam: bad upstream response\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkt(line: &str) -> String {
        format!("{:04x}{line}", line.len() + 4)
    }

    const A: &str = "1111111111111111111111111111111111111111";
    const B: &str = "2222222222222222222222222222222222222222";
    const Z: &str = "0000000000000000000000000000000000000000";

    #[test]
    fn parses_ref_updates_before_the_pack() {
        let mut body = String::new();
        body.push_str(&pkt(&format!("{A} {B} refs/heads/main\0 report-status side-band-64k agent=git/2.34\n")));
        body.push_str(&pkt(&format!("{B} {Z} refs/heads/old\n")));
        body.push_str("0000PACK....");
        let Parsed::Done(updates, offset) = parse_push_commands(body.as_bytes()) else { panic!("expected Done") };
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].name, "refs/heads/main");
        assert!(!updates[0].is_delete());
        assert!(updates[1].is_delete());
        assert_eq!(&body.as_bytes()[offset..offset + 4], b"PACK");
    }

    #[test]
    fn asks_for_more_until_flush() {
        let full = format!("{}0000", pkt(&format!("{A} {B} refs/heads/x\n")));
        for cut in [0, 3, 10, full.len() - 1] {
            assert_eq!(parse_push_commands(&full.as_bytes()[..cut]), Parsed::NeedMore, "cut at {cut}");
        }
        assert!(matches!(parse_push_commands(full.as_bytes()), Parsed::Done(..)));
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(parse_push_commands(b"zzzz"), Parsed::Invalid(_)));
        assert!(matches!(parse_push_commands(b"0003"), Parsed::Invalid(_)));
        assert!(matches!(parse_push_commands(pkt("hello\n").as_bytes()), Parsed::Invalid(_)));
    }

    #[test]
    fn splits_paths() {
        assert_eq!(split_path("acme/api.git/info/refs"), Some(("acme/api".to_string(), "info/refs")));
        assert_eq!(split_path("acme/api/git-upload-pack"), Some(("acme/api".to_string(), "git-upload-pack")));
        assert_eq!(split_path("acme/api.git"), None);
        assert_eq!(split_path("../x/info/refs"), None);
    }
}
