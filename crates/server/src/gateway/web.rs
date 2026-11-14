//! Reading the public web on an agent's behalf.
//!
//! A sandbox has no route to the internet beyond the egress allowlist, and
//! opening one would hand any prompt injection a way to send out what the
//! sandbox holds. `web_fetch` is the alternative: the control plane fetches
//! the page, so every request is checked, audited and shown in the session
//! log like any other gateway call.
//!
//! A GET is still a message to whoever runs the server, and an agent that
//! could fetch any URL could put secrets in one. So by default an agent may
//! only fetch URLs it did not write itself: ones a person gave it, ones a web
//! search returned, and ones a page it fetched links to. Admins can list
//! sites agents may read freely, restrict agents to that list, let agents
//! read any public page, or turn web access off.

use super::{ToolDef, ToolError, ToolOutput, ToolResult, arg_str, schema};
use crate::app::App;
use crate::egress::host_allowed;
use crate::store::{self, Session};
use hyperbeam_core::Grant;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use url::Url;
use uuid::Uuid;

/// Longest URL an agent may fetch or that is remembered.
const MAX_URL_CHARS: usize = 2000;
/// Most of a response that is read; the rest is dropped.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// Most text returned by one call; `offset` reads on from there.
const PAGE_CHARS: usize = 40_000;
const MAX_REDIRECTS: usize = 5;
/// URLs remembered from one page, search result or message.
const MAX_URLS_PER_SOURCE: usize = 500;
/// Fetches a session may make in a minute.
const FETCHES_PER_MINUTE: i64 = 30;
const TIMEOUT: Duration = Duration::from_secs(20);

/// Which pages agents may read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Any public URL, including ones the agent wrote itself.
    Any,
    /// URLs the agent was given or found, plus the listed sites.
    #[default]
    Known,
    /// Only the listed sites.
    Listed,
    Off,
}

/// The team's `web_access` setting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    #[serde(default)]
    pub mode: Mode,
    /// Sites agents may read any page of: `example.com` or `*.example.com`.
    #[serde(default)]
    pub domains: Vec<String>,
}

impl Policy {
    pub async fn load(app: &App) -> sqlx::Result<Policy> {
        let stored = store::get_setting(&app.db, "web_access").await?;
        Ok(stored.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
    }

    fn lists(&self, url: &Url) -> bool {
        url.host_str().is_some_and(|host| host_allowed(host, &self.domains))
    }
}

pub fn tools() -> Vec<ToolDef> {
    vec![ToolDef {
        name: "web_fetch",
        description: "Read a public web page (documentation, an article, a raw file) as markdown. \
                      Which URLs you may fetch is set by the team; a refused fetch says why. \
                      Long pages come in parts: pass `offset` to read on.",
        schema: schema(&[
            ("url", "string", "The http(s) URL to read", true),
            ("offset", "integer", "Character to start from, for reading on through a long page", false),
        ]),
        grant: Some(Grant::WEB_READ),
        conductor_only: false,
    }]
}

pub async fn call(app: &App, session: &Session, name: &str, args: &Value) -> ToolResult {
    match name {
        "web_fetch" => fetch(app, session, args).await,
        other => Err(ToolError::Failed(format!("no tool named `{other}`"))),
    }
}

// ------------------------------------------------------------------- policy

/// Why a URL may be fetched, which decides where redirects may lead.
#[derive(Clone, Copy, PartialEq)]
enum Basis {
    /// The agent did not write it, so neither did it choose where it leads.
    Known,
    /// It is on a listed site, and the agent may have written the rest.
    Listed,
    /// The team lets agents read any public page.
    Any,
}

fn parse(raw: &str) -> Result<Url, ToolError> {
    if raw.chars().count() > MAX_URL_CHARS {
        return Err(ToolError::Failed(format!("URL is too long (limit {MAX_URL_CHARS} characters)")));
    }
    let url = Url::parse(raw).map_err(|_| ToolError::Failed(format!("`{raw}` is not a URL")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(ToolError::Failed("only http and https URLs can be fetched".into()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ToolError::Denied("URLs that carry a username or password are not fetched".into()));
    }
    Ok(url)
}

/// What a URL is remembered and matched as: its host, path and query. The
/// scheme, a default port, the fragment and a trailing slash do not matter.
fn url_key(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let mut key = host.to_ascii_lowercase();
    if let Some(port) = url.port() {
        key.push_str(&format!(":{port}"));
    }
    key.push_str(url.path().trim_end_matches('/'));
    if let Some(query) = url.query().filter(|q| !q.is_empty()) {
        key.push('?');
        key.push_str(query);
    }
    (key.chars().count() <= MAX_URL_CHARS).then_some(key)
}

/// The http(s) URLs written out in a piece of text.
pub fn extract_urls(text: &str) -> Vec<Url> {
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find("http") {
        let candidate = &rest[start..];
        let boundary = rest[..start].chars().next_back().is_none_or(|c| !c.is_ascii_alphanumeric());
        if !boundary || !(candidate.starts_with("http://") || candidate.starts_with("https://")) {
            rest = &candidate[4..];
            continue;
        }
        let end = candidate
            .find(|c: char| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | '`' | '\\' | '^' | '{' | '}' | '|'))
            .unwrap_or(candidate.len());
        let mut raw = &candidate[..end];
        // Punctuation that ends a sentence or closes a bracket around the
        // URL is not part of it.
        loop {
            let trimmed = raw.trim_end_matches(['.', ',', ';', ':', '!', '?', '\'', '*', '_']);
            let trimmed = match trimmed.chars().next_back() {
                Some(')') if trimmed.matches(')').count() > trimmed.matches('(').count() => &trimmed[..trimmed.len() - 1],
                Some(']') if trimmed.matches(']').count() > trimmed.matches('[').count() => &trimmed[..trimmed.len() - 1],
                _ => trimmed,
            };
            if trimmed.len() == raw.len() {
                break;
            }
            raw = trimmed;
        }
        if let Ok(url) = Url::parse(raw)
            && url.host_str().is_some_and(|h| h.contains('.') || h.parse::<IpAddr>().is_ok() || h == "localhost")
            && let Some(key) = url_key(&url)
            && seen.insert(key)
        {
            found.push(url);
        }
        rest = &candidate[end.max(4)..];
    }
    found
}

/// The session at the top of this one's tree. A conductor and its workers
/// share what they may fetch: none of it was written by an agent.
async fn root_of(app: &App, session: &Session) -> sqlx::Result<Uuid> {
    let (mut root, mut parent) = (session.id, session.parent_id);
    for _ in 0..16 {
        let Some(id) = parent else { break };
        let row: Option<(Option<Uuid>,)> = sqlx::query_as("SELECT parent_id FROM sessions WHERE id = $1").bind(id).fetch_optional(&app.db).await?;
        let Some((next,)) = row else { break };
        root = id;
        parent = next;
    }
    Ok(root)
}

/// Remember URLs this session's tree may fetch. `source` says where they
/// came from: `person`, `search` or `page`.
async fn remember(app: &App, session: &Session, urls: &[Url], source: &str) -> sqlx::Result<()> {
    let keys: Vec<String> = urls.iter().filter_map(url_key).take(MAX_URLS_PER_SOURCE).collect();
    if keys.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO web_urls (root_session_id, url, source)
         SELECT $1, key, $3 FROM unnest($2::text[]) AS key ON CONFLICT DO NOTHING",
    )
    .bind(root_of(app, session).await?)
    .bind(&keys)
    .bind(source)
    .execute(&app.db)
    .await?;
    Ok(())
}

/// Remember the URLs in text that the agent did not write: a person's
/// message, or the results of a web search.
pub async fn remember_from_text(app: &App, session: &Session, text: &str, source: &str) {
    if !text.contains("http") {
        return;
    }
    if let Err(err) = remember(app, session, &extract_urls(text), source).await {
        tracing::warn!("could not record URLs for session {}: {err}", session.id);
    }
}

/// Was this URL given to the agent, or found by it, rather than written by it?
async fn known(app: &App, session: &Session, url: &Url) -> sqlx::Result<bool> {
    let Some(key) = url_key(url) else { return Ok(false) };
    let (remembered,): (bool,) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM web_urls WHERE root_session_id = $1 AND url = $2)")
        .bind(root_of(app, session).await?)
        .bind(&key)
        .fetch_one(&app.db)
        .await?;
    if remembered {
        return Ok(true);
    }
    // People also write URLs into a workstream's brief and into skills.
    let mut written: Vec<String> = sqlx::query_as::<_, (String,)>("SELECT content FROM skills WHERE name = ANY($1)")
        .bind(&session.skills.0)
        .fetch_all(&app.db)
        .await?
        .into_iter()
        .map(|(content,)| content)
        .collect();
    if let Some(ws) = session.workstream_id {
        let brief: Option<(String,)> = sqlx::query_as("SELECT description FROM workstreams WHERE id = $1").bind(ws).fetch_optional(&app.db).await?;
        written.extend(brief.map(|(description,)| description));
    }
    Ok(written.iter().flat_map(|text| extract_urls(text)).any(|u| url_key(&u).as_deref() == Some(key.as_str())))
}

async fn permitted(app: &App, session: &Session, policy: &Policy, url: &Url) -> Result<Basis, ToolError> {
    let host = url.host_str().unwrap_or_default();
    match policy.mode {
        Mode::Off => Err(ToolError::Denied("web access is turned off for this team".into())),
        Mode::Any => Ok(Basis::Any),
        _ if policy.lists(url) => Ok(Basis::Listed),
        Mode::Listed => Err(ToolError::Denied(format!(
            "`{host}` is not one of the sites this team lets agents read. An admin can add it under Settings → General."
        ))),
        Mode::Known if known(app, session, url).await? => Ok(Basis::Known),
        Mode::Known => Err(ToolError::Denied(
            "you can fetch URLs a person gave you, URLs a web search returned, and links on pages you have fetched, \
             but not URLs you put together yourself. Find the page through a search or a link, or ask for it with `hyperbeam_ask_user`."
                .into(),
        )),
    }
}

// -------------------------------------------------------------------- fetch

/// Is this an address on the public internet? Everything else (loopback,
/// private networks, link-local and cloud metadata, carrier-grade NAT,
/// multicast) is somewhere the control plane can reach and an agent must not.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 198 && (b == 18 || b == 19))
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || (first == 0x2001 && v6.segments()[1] == 0x0db8)
                || (first == 0x0064 && v6.segments()[1] == 0xff9b))
        }
    }
}

/// Resolve a URL's host to the address the request will go to, refusing
/// anything that is not on the public internet. The request is then pinned
/// to that address, so a name cannot resolve one way here and another there.
async fn vetted_address(app: &App, url: &Url) -> Result<SocketAddr, ToolError> {
    let host = url.host_str().unwrap_or_default();
    let port = url.port_or_known_default().unwrap_or(443);
    let lookup = host.trim_matches(['[', ']']);
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((lookup, port))
        .await
        .map_err(|_| ToolError::Failed(format!("could not find `{host}`")))?
        .collect();
    if addrs.is_empty() {
        return Err(ToolError::Failed(format!("could not find `{host}`")));
    }
    if !app.cfg.web_allow_private && !addrs.iter().all(|addr| is_public(addr.ip())) {
        return Err(ToolError::Denied(format!("`{host}` is not on the public internet")));
    }
    Ok(addrs[0])
}

struct Fetched {
    url: Url,
    content_type: String,
    body: Vec<u8>,
    cut_short: bool,
}

async fn get(app: &App, session: &Session, policy: &Policy, start: Url, basis: Basis) -> Result<Fetched, ToolError> {
    let mut url = start;
    for _ in 0..=MAX_REDIRECTS {
        let addr = vetted_address(app, &url).await?;
        let host = url.host_str().unwrap_or_default().to_string();
        let client = reqwest::Client::builder()
            .user_agent(concat!("hyperbeam-web-fetch/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .resolve(&host, addr)
            .connect_timeout(Duration::from_secs(10))
            .timeout(TIMEOUT)
            .build()
            .map_err(|err| ToolError::Failed(format!("could not prepare the request: {err}")))?;
        let mut response = client
            .get(url.clone())
            .header("accept", "text/markdown, text/html;q=0.9, text/plain;q=0.8, application/json;q=0.8, */*;q=0.1")
            .send()
            .await
            .map_err(|err| ToolError::Failed(format!("could not reach {host}: {}", err.without_url())))?;

        let status = response.status();
        if status.is_redirection() {
            let Some(location) = response.headers().get("location").and_then(|v| v.to_str().ok()) else {
                return Err(ToolError::Failed(format!("{host} answered {status} without saying where to go")));
            };
            let next = parse(url.join(location).map_err(|_| ToolError::Failed(format!("{host} redirected somewhere that is not a URL")))?.as_str())?;
            // A URL the agent may have written must not be able to bounce
            // off a listed site to one that is not.
            if basis == Basis::Listed && !policy.lists(&next) && !known(app, session, &next).await? {
                return Err(ToolError::Denied(format!(
                    "{host} redirected to `{}`, which is not one of the sites this team lets agents read",
                    next.host_str().unwrap_or_default()
                )));
            }
            url = next;
            continue;
        }
        if !status.is_success() {
            return Err(ToolError::Failed(format!("{host} answered {status}")));
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/plain")
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if !is_text(&content_type) {
            return Err(ToolError::Failed(format!("that is a `{content_type}` file, not a page of text")));
        }
        let mut body = Vec::new();
        let mut cut_short = false;
        while let Some(chunk) = response.chunk().await.map_err(|err| ToolError::Failed(format!("{host} stopped answering: {}", err.without_url())))? {
            let room = MAX_BODY_BYTES - body.len();
            if chunk.len() > room {
                body.extend_from_slice(&chunk[..room]);
                cut_short = true;
                break;
            }
            body.extend_from_slice(&chunk);
        }
        return Ok(Fetched { url, content_type, body, cut_short });
    }
    Err(ToolError::Failed("too many redirects".into()))
}

fn is_text(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || content_type.ends_with("+json")
        || content_type.ends_with("+xml")
        || matches!(content_type, "application/json" | "application/xml" | "application/javascript" | "application/x-yaml" | "application/yaml" | "application/toml")
}

/// A page's `<title>`, if it has one.
fn title_of(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let start = open + lower[open..].find('>')? + 1;
    let end = start + lower[start..].find("</title")?;
    let title = html.get(start..end)?.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = title.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'");
    (!title.is_empty()).then_some(title)
}

fn to_markdown(html: &str) -> String {
    let converter = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "noscript", "template", "svg", "iframe", "head", "nav", "footer", "form", "button"])
        .build();
    converter.convert(html).unwrap_or_default()
}

/// Rewrite the targets of markdown links against the page's own address, so
/// a relative link reads, and can be fetched, as the URL it stands for.
fn absolutize(markdown: &str, base: &Url) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut rest = markdown;
    while let Some(at) = rest.find("](") {
        let (before, after) = rest.split_at(at + 2);
        out.push_str(before);
        let Some(close) = after.find(')') else {
            rest = after;
            break;
        };
        let inside = &after[..close];
        let (target, title) = match inside.find(char::is_whitespace) {
            Some(space) => inside.split_at(space),
            None => (inside, ""),
        };
        let bare = target.trim_start_matches('<').trim_end_matches('>');
        match base.join(bare) {
            Ok(joined) if !bare.is_empty() && !bare.starts_with('#') && matches!(joined.scheme(), "http" | "https") => {
                out.push_str(joined.as_str());
                out.push_str(title);
            }
            _ => out.push_str(inside),
        }
        rest = &after[close..];
    }
    out.push_str(rest);
    out
}

async fn fetch(app: &App, session: &Session, args: &Value) -> ToolResult {
    let raw = arg_str(args, "url")?;
    let offset = args.get("offset").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).unwrap_or(0) as usize;
    let url = parse(raw)?;
    let policy = Policy::load(app).await?;
    let basis = permitted(app, session, &policy, &url).await?;

    let (recent,): (i64,) = sqlx::query_as("SELECT count(*) FROM audit WHERE session_id = $1 AND action = 'web_fetch' AND ts > now() - interval '1 minute'")
        .bind(session.id)
        .fetch_one(&app.db)
        .await?;
    if recent >= FETCHES_PER_MINUTE {
        return Err(ToolError::Failed(format!("this session has fetched {recent} pages in the last minute; wait a little before fetching more")));
    }

    let page = get(app, session, &policy, url, basis).await?;
    let html = matches!(page.content_type.as_str(), "text/html" | "application/xhtml+xml");
    let source = String::from_utf8_lossy(&page.body).into_owned();
    let base = page.url.clone();
    let (title, text) = if html {
        // Parsing a large page is real work; keep it off the async threads.
        tokio::task::spawn_blocking(move || (title_of(&source), absolutize(&to_markdown(&source), &base)))
            .await
            .map_err(|_| ToolError::Failed("could not read that page".into()))?
    } else {
        (None, source)
    };
    let text = text.trim();

    // Whatever the page links to, the agent may follow.
    let mut found = extract_urls(text);
    found.push(page.url.clone());
    remember(app, session, &found, "page").await?;

    let total = text.chars().count();
    let shown: String = text.chars().skip(offset).take(PAGE_CHARS).collect();
    let end = offset + shown.chars().count();
    let mut out = String::new();
    if let Some(title) = title {
        out.push_str(&format!("# {title}\n"));
    }
    out.push_str(&format!("Source: {}\n\n", page.url));
    if shown.is_empty() {
        out.push_str(if total == 0 { "(The page has no readable text. It may need JavaScript to show anything.)" } else { "(Nothing further: the page ends before this offset.)" });
    } else {
        out.push_str(&shown);
    }
    if end < total {
        out.push_str(&format!("\n\n[Characters {offset} to {end} of {total}. Call again with offset {end} to read on.]"));
    } else if page.cut_short {
        out.push_str("\n\n[The page was larger than can be fetched; this is the first part of it.]");
    }
    Ok(ToolOutput::new(page.url.to_string(), out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn urls(text: &str) -> Vec<String> {
        extract_urls(text).iter().map(Url::to_string).collect()
    }

    #[test]
    fn finds_urls_in_prose_and_markdown() {
        assert_eq!(
            urls("See https://docs.rs/tokio/latest/tokio/, then (https://example.com/a_(b)) and [the guide](https://example.com/guide?x=1&y=2)."),
            ["https://docs.rs/tokio/latest/tokio/", "https://example.com/a_(b)", "https://example.com/guide?x=1&y=2"]
        );
        assert_eq!(urls("<https://example.com/x> \"http://example.com/y\" `https://example.com/z`"), ["https://example.com/x", "http://example.com/y", "https://example.com/z"]);
        // Not URLs, and the same page twice is one page.
        assert!(urls("xhttps://example.com https:// http://nodot ftp://example.com").is_empty());
        assert_eq!(urls("https://example.com/a https://example.com/a/ http://EXAMPLE.com/a#top").len(), 1);
    }

    #[test]
    fn urls_match_on_host_path_and_query_only() {
        let key = |s: &str| url_key(&Url::parse(s).unwrap()).unwrap();
        assert_eq!(key("https://Example.com/docs/"), "example.com/docs");
        assert_eq!(key("http://example.com:443/docs#intro"), "example.com:443/docs");
        assert_eq!(key("https://example.com:443/"), "example.com");
        assert_ne!(key("https://example.com/docs?page=2"), key("https://example.com/docs"));
        assert_ne!(key("https://example.com/docs?secret=hunter2"), key("https://example.com/docs"));
    }

    #[test]
    fn only_public_addresses_are_fetched() {
        for private in [
            "127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.1", "169.254.169.254", "100.64.0.1", "0.0.0.0", "198.18.0.1", "224.0.0.1", "255.255.255.255",
            "::1", "::", "fc00::1", "fd12:3456::1", "fe80::1", "::ffff:10.0.0.1", "::ffff:127.0.0.1", "64:ff9b::a00:1",
        ] {
            assert!(!is_public(private.parse().unwrap()), "{private}");
        }
        for public in ["1.1.1.1", "93.184.216.34", "172.32.0.1", "100.128.0.1", "2606:4700:4700::1111", "::ffff:1.1.1.1"] {
            assert!(is_public(public.parse().unwrap()), "{public}");
        }
    }

    #[test]
    fn urls_are_checked_before_anything_is_fetched() {
        assert!(parse("https://example.com/a").is_ok());
        for bad in ["file:///etc/passwd", "ftp://example.com/x", "example.com", "https://user:pw@example.com/", "https://token@example.com/"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert!(parse(&format!("https://example.com/{}", "a".repeat(MAX_URL_CHARS))).is_err());
    }

    #[test]
    fn pages_become_markdown_with_absolute_links() {
        let html = r##"<html><head><title> Getting  started &amp; more </title><style>p{}</style></head>
            <body><nav><a href="/skip">Skip</a></nav><h1>Guide</h1><p>Read <a href="../api/index.html">the API</a>, <a href="https://other.example/x">elsewhere</a>
            or <a href="#top">the top</a>.</p><script>alert(1)</script></body></html>"##;
        assert_eq!(title_of(html).as_deref(), Some("Getting started & more"));
        let base = Url::parse("https://example.com/docs/guide/").unwrap();
        let markdown = absolutize(&to_markdown(html), &base);
        assert!(markdown.contains("# Guide"), "{markdown}");
        assert!(markdown.contains("[the API](https://example.com/docs/api/index.html)"), "{markdown}");
        assert!(markdown.contains("(https://other.example/x)") && markdown.contains("(#top)"), "{markdown}");
        assert!(!markdown.contains("alert") && !markdown.contains("Skip"), "{markdown}");
        assert_eq!(absolutize("[a](b \"t\") and ](", &base), "[a](https://example.com/docs/guide/b \"t\") and ](");
    }

    #[test]
    fn the_setting_defaults_to_known_urls() {
        assert_eq!(Policy::default().mode, Mode::Known);
        let policy: Policy = serde_json::from_value(json!({ "mode": "listed", "domains": ["docs.rs", "*.example.com"] })).unwrap();
        assert_eq!(policy.mode, Mode::Listed);
        assert!(policy.lists(&Url::parse("https://docs.rs/tokio").unwrap()));
        assert!(policy.lists(&Url::parse("https://a.example.com/").unwrap()));
        assert!(!policy.lists(&Url::parse("https://example.com/").unwrap()));
        let policy: Policy = serde_json::from_value(json!({ "mode": "any" })).unwrap();
        assert_eq!(policy.mode, Mode::Any);
    }
}
