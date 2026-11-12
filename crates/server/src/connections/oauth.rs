//! OAuth sign-in for connections, as the MCP authorization spec describes it:
//! the authorization-code flow with PKCE, found by discovery from the server's
//! URL, with tokens refreshed as they expire.
//!
//! An admin signs in once and every agent acts as that account. The client is
//! one the admin supplies, or one the server registers for us when it allows
//! that (RFC 7591). Tokens are stored like any other credential: encrypted,
//! in the control plane, never in a sandbox.

use super::{Auth, Connection, by_name, secret_key};
use crate::app::App;
use crate::store;
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use rand::RngCore;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::types::Json;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use url::Url;
use uuid::Uuid;

/// A sign-in must be finished within this long of being started.
const FLOW_LIFETIME: Duration = Duration::from_secs(10 * 60);
/// A token this close to expiring is refreshed before use.
const REFRESH_MARGIN_SECS: i64 = 60;
const TIMEOUT: Duration = Duration::from_secs(20);

/// How the client identifies itself at the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientAuth {
    /// A public client: its id only.
    None,
    /// Id and secret in an `Authorization: Basic` header.
    Basic,
    /// Id and secret in the form body.
    Post,
}

/// What a connection needs to keep its sign-in alive. Nothing here is secret:
/// the tokens and any client secret live in `secrets`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth {
    pub client_id: String,
    pub client_auth: ClientAuth,
    /// The server registered the client for us, rather than an admin supplying it.
    #[serde(default)]
    pub registered: bool,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub scope: Option<String>,
    /// The server the tokens are for (RFC 8707).
    pub resource: String,
    /// When the access token expires, if the server said.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

impl OAuth {
    fn expiring(&self) -> bool {
        self.expires_at.is_some_and(|at| (at - Utc::now()).num_seconds() < REFRESH_MARGIN_SECS)
    }
}

pub fn refresh_key(name: &str) -> String {
    format!("connection/{name}/refresh")
}

pub fn client_secret_key(name: &str) -> String {
    format!("connection/{name}/client")
}

/// Forget everything a sign-in stored except the access token.
pub async fn delete_secrets(db: &sqlx::PgPool, name: &str) -> sqlx::Result<()> {
    store::delete_secret(db, &refresh_key(name)).await?;
    store::delete_secret(db, &client_secret_key(name)).await
}

/// The connection a sign-in will create or update once it succeeds.
#[derive(Debug, Clone)]
pub struct Spec {
    pub name: String,
    pub preset: Option<String>,
    pub title: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub options: Value,
}

/// An OAuth client an admin created with the server's vendor.
#[derive(Debug, Clone)]
pub struct Client {
    pub id: String,
    pub secret: Option<String>,
}

/// A sign-in that has been sent to the server and not yet come back.
pub struct Pending {
    pub spec: Spec,
    pub oauth: OAuth,
    pub client_secret: Option<String>,
    pub actor: Uuid,
    verifier: String,
    redirect_uri: String,
    started: Instant,
}

pub struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// What discovery learned about a server's authorization server.
#[derive(Debug)]
struct Discovered {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
    /// Token-endpoint client authentication methods on offer.
    auth_methods: Vec<String>,
    scope: Option<String>,
    resource: String,
}

fn random(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    URL_SAFE_NO_PAD.encode(buffer)
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// A parameter of a `WWW-Authenticate` challenge: `name="value"`.
fn challenge_param(header: &str, name: &str) -> Option<String> {
    let mut rest = header;
    while let Some(at) = rest.find(name) {
        let boundary = at == 0 || !rest[..at].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
        let after = rest[at + name.len()..].trim_start();
        if boundary && let Some(value) = after.strip_prefix('=') {
            let value = value.trim_start();
            return Some(match value.strip_prefix('"') {
                Some(quoted) => quoted.split('"').next().unwrap_or_default().to_string(),
                None => value.split([',', ' ']).next().unwrap_or_default().to_string(),
            })
            .filter(|v| !v.is_empty());
        }
        rest = &rest[at + name.len()..];
    }
    None
}

fn http_url(raw: &str, what: &str) -> Result<String> {
    let url = Url::parse(raw).map_err(|_| anyhow!("its {what} (`{raw}`) is not a URL"))?;
    if !matches!(url.scheme(), "https" | "http") || url.host_str().is_none() {
        bail!("its {what} (`{raw}`) is not an http(s) URL");
    }
    Ok(url.to_string())
}

async fn get_json(http: &reqwest::Client, url: &str) -> Option<Value> {
    let response = http.get(url).header("accept", "application/json").timeout(TIMEOUT).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<Value>().await.ok().filter(Value::is_object)
}

/// `https://host/.well-known/<name><path>`: where RFC 8414 and RFC 9728 put
/// metadata for an identifier that has a path.
fn well_known(base: &Url, name: &str, with_path: bool) -> String {
    let mut url = base.clone();
    url.set_query(None);
    url.set_fragment(None);
    let path = if with_path { base.path().trim_end_matches('/') } else { "" };
    url.set_path(&format!("/.well-known/{name}{path}"));
    url.to_string()
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect()
}

/// Find a server's authorization server, starting from its MCP URL.
async fn discover(http: &reqwest::Client, label: &str, mcp_url: &str) -> Result<Discovered> {
    let url = Url::parse(mcp_url).with_context(|| format!("`{mcp_url}` is not a URL"))?;

    // An unauthenticated request should be refused with a pointer to the
    // server's metadata and, perhaps, the scope to ask for.
    let probe = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "hyperbeam", "version": env!("CARGO_PKG_VERSION") } },
    });
    let refused = http
        .post(mcp_url)
        .header("accept", "application/json, text/event-stream")
        .json(&probe)
        .timeout(TIMEOUT)
        .send()
        .await
        .with_context(|| format!("could not reach {label}"))?;
    let header = refused.headers().get(WWW_AUTHENTICATE).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    let pointed = challenge_param(&header, "resource_metadata").filter(|u| Url::parse(u).is_ok_and(|u| matches!(u.scheme(), "https" | "http")));

    let mut resource_docs: Vec<String> = pointed.into_iter().collect();
    resource_docs.push(well_known(&url, "oauth-protected-resource", true));
    resource_docs.push(well_known(&url, "oauth-protected-resource", false));
    let mut resource_meta = None;
    for doc in &resource_docs {
        if let Some(meta) = get_json(http, doc).await {
            resource_meta = Some(meta);
            break;
        }
    }

    // Servers from before protected-resource metadata are their own
    // authorization server.
    let issuer = resource_meta
        .as_ref()
        .and_then(|m| strings(m.get("authorization_servers")).into_iter().next())
        .and_then(|issuer| Url::parse(&issuer).ok())
        .unwrap_or_else(|| {
            let mut origin = url.clone();
            origin.set_path("/");
            origin.set_query(None);
            origin.set_fragment(None);
            origin
        });
    let has_path = !issuer.path().trim_end_matches('/').is_empty();
    let mut server_docs = vec![well_known(&issuer, "oauth-authorization-server", has_path), well_known(&issuer, "openid-configuration", has_path)];
    if has_path {
        server_docs.push(format!("{}/.well-known/openid-configuration", issuer.as_str().trim_end_matches('/')));
    }
    let mut server_meta = None;
    for doc in &server_docs {
        if let Some(meta) = get_json(http, doc).await.filter(|m| m.get("token_endpoint").is_some()) {
            server_meta = Some(meta);
            break;
        }
    }
    let meta = server_meta.ok_or_else(|| anyhow!("{label} does not describe an OAuth sign-in (no authorization server metadata at {})", server_docs[0]))?;

    let endpoint = |key: &str, what: &str| -> Result<String> {
        let raw = meta.get(key).and_then(Value::as_str).ok_or_else(|| anyhow!("{label}'s authorization server does not name its {what}"))?;
        http_url(raw, what).with_context(|| format!("{label}'s authorization server"))
    };
    if !strings(meta.get("code_challenge_methods_supported")).iter().any(|m| m == "S256") {
        bail!("{label}'s authorization server does not offer PKCE (S256), which Hyperbeam requires");
    }
    let methods = strings(meta.get("token_endpoint_auth_methods_supported"));
    let scope = challenge_param(&header, "scope").or_else(|| {
        let listed = strings(resource_meta.as_ref().and_then(|m| m.get("scopes_supported")));
        (!listed.is_empty()).then(|| listed.join(" "))
    });
    let mut canonical = url.clone();
    canonical.set_fragment(None);
    Ok(Discovered {
        authorization_endpoint: endpoint("authorization_endpoint", "authorization endpoint")?,
        token_endpoint: endpoint("token_endpoint", "token endpoint")?,
        registration_endpoint: meta.get("registration_endpoint").and_then(Value::as_str).and_then(|raw| http_url(raw, "registration endpoint").ok()),
        // RFC 8414: `client_secret_basic` when the server does not say.
        auth_methods: if methods.is_empty() { vec!["client_secret_basic".into()] } else { methods },
        scope,
        resource: resource_meta.as_ref().and_then(|m| m.get("resource")).and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| canonical.to_string()),
    })
}

/// Have the server register a client for this installation (RFC 7591).
async fn register(http: &reqwest::Client, label: &str, endpoint: &str, redirect_uri: &str, scope: Option<&str>) -> Result<(Client, ClientAuth)> {
    let mut request = json!({
        "client_name": "Hyperbeam",
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    if let Some(scope) = scope {
        request["scope"] = json!(scope);
    }
    let response = http.post(endpoint).json(&request).timeout(TIMEOUT).send().await.with_context(|| format!("could not reach {label} to register"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        bail!("{label} would not register Hyperbeam as a client ({status}{})", problem(&body));
    }
    let id = body.get("client_id").and_then(Value::as_str).ok_or_else(|| anyhow!("{label} registered a client without an id"))?.to_string();
    let secret = body.get("client_secret").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let auth = match (&secret, body.get("token_endpoint_auth_method").and_then(Value::as_str)) {
        (None, _) => ClientAuth::None,
        (Some(_), Some("client_secret_post")) => ClientAuth::Post,
        (Some(_), _) => ClientAuth::Basic,
    };
    Ok((Client { id, secret }, auth))
}

/// `: description` from an OAuth error body, if there is one.
fn problem(body: &Value) -> String {
    let text = body.get("error_description").or_else(|| body.get("error")).and_then(Value::as_str).unwrap_or_default();
    if text.is_empty() { String::new() } else { format!(": {}", text.chars().take(300).collect::<String>()) }
}

async fn token_request(http: &reqwest::Client, label: &str, oauth: &OAuth, client_secret: Option<&str>, grant: &[(&str, &str)]) -> Result<Tokens> {
    let mut form: Vec<(&str, &str)> = grant.to_vec();
    form.push(("resource", &oauth.resource));
    let mut request = http.post(&oauth.token_endpoint).header("accept", "application/json").header(CONTENT_TYPE, "application/x-www-form-urlencoded").timeout(TIMEOUT);
    match (oauth.client_auth, client_secret) {
        (ClientAuth::Basic, Some(secret)) => {
            // RFC 6749 §2.3.1: both parts are form-encoded before base64.
            let encode = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
            let pair = format!("{}:{}", encode(&oauth.client_id), encode(secret));
            request = request.header(AUTHORIZATION, format!("Basic {}", STANDARD.encode(pair)));
        }
        (ClientAuth::Post, Some(secret)) => form.extend([("client_id", oauth.client_id.as_str()), ("client_secret", secret)]),
        _ => form.push(("client_id", &oauth.client_id)),
    }
    let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish();
    let response = request.body(body).send().await.with_context(|| format!("could not reach {label}'s token endpoint"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_default();
    if !status.is_success() || body.get("error").is_some() {
        bail!("{label} would not issue a token ({status}{})", problem(&body));
    }
    let access = body.get("access_token").and_then(Value::as_str).filter(|t| !t.is_empty()).ok_or_else(|| anyhow!("{label} answered without an access token"))?;
    // Slack calls the bearer token it issues for a person a `user` token.
    if body.get("token_type").and_then(Value::as_str).is_some_and(|t| !t.eq_ignore_ascii_case("bearer") && t != "user") {
        bail!("{label} issued a token Hyperbeam cannot use (type `{}`)", body["token_type"].as_str().unwrap_or_default());
    }
    if access.contains(['\r', '\n']) {
        bail!("{label} issued a malformed token");
    }
    let lifetime = body.get("expires_in").and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
    Ok(Tokens {
        access: access.to_string(),
        refresh: body.get("refresh_token").and_then(Value::as_str).filter(|t| !t.is_empty()).map(str::to_string),
        expires_at: lifetime.filter(|s| *s > 0).map(|s| Utc::now() + chrono::Duration::seconds(s)),
    })
}

impl App {
    /// Where authorization servers send people back to.
    pub fn oauth_redirect_uri(&self) -> String {
        format!("{}/api/v1/oauth/callback", self.cfg.public_url.trim_end_matches('/'))
    }

    /// Start a sign-in for the connection `spec` describes. Returns the URL
    /// to send the admin's browser to. Nothing is saved until they come back.
    ///
    /// The client is `client` if an admin supplied one, else the one
    /// `existing` already uses, else one the server registers for us.
    pub async fn begin_oauth(&self, spec: Spec, client: Option<Client>, existing: Option<&Connection>, scope: Option<String>, actor: Uuid) -> Result<String> {
        let label = spec.title.clone();
        let found = discover(&self.http, &label, &spec.url).await?;
        let redirect_uri = self.oauth_redirect_uri();
        let scope = scope.or(found.scope);
        let kept = match existing.and_then(|c| c.auth.oauth.as_ref().map(|o| (c, o))) {
            // A client is only good at the authorization server that knows it.
            Some((conn, oauth)) if client.is_none() && oauth.token_endpoint == found.token_endpoint => {
                let secret = store::get_secret(&self.db, &self.secrets, &client_secret_key(&conn.name)).await?;
                Some((Client { id: oauth.client_id.clone(), secret }, oauth.client_auth, oauth.registered))
            }
            _ => None,
        };
        let (client, client_auth, registered) = match (client, kept, &found.registration_endpoint) {
            (Some(client), _, _) => {
                let offers = |method: &str| found.auth_methods.iter().any(|m| m == method);
                let auth = match &client.secret {
                    None => ClientAuth::None,
                    Some(_) if offers("client_secret_basic") || !offers("client_secret_post") => ClientAuth::Basic,
                    Some(_) => ClientAuth::Post,
                };
                (client, auth, false)
            }
            (None, Some(kept), _) => kept,
            (None, None, Some(endpoint)) => {
                let (client, auth) = register(&self.http, &label, endpoint, &redirect_uri, scope.as_deref()).await?;
                (client, auth, true)
            }
            (None, None, None) => bail!(
                "{label} does not let clients register themselves. Create an OAuth client with {label}, with the redirect URL {redirect_uri}, and enter its client ID."
            ),
        };

        let state = random(32);
        let verifier = random(32);
        let mut authorize = Url::parse(&found.authorization_endpoint)?;
        {
            let mut query = authorize.query_pairs_mut();
            query
                .append_pair("response_type", "code")
                .append_pair("client_id", &client.id)
                .append_pair("redirect_uri", &redirect_uri)
                .append_pair("state", &state)
                .append_pair("code_challenge", &challenge(&verifier))
                .append_pair("code_challenge_method", "S256")
                .append_pair("resource", &found.resource);
            if let Some(scope) = &scope {
                query.append_pair("scope", scope);
            }
        }
        let pending = Pending {
            spec,
            oauth: OAuth {
                client_id: client.id,
                client_auth,
                registered,
                authorization_endpoint: found.authorization_endpoint,
                token_endpoint: found.token_endpoint,
                scope,
                resource: found.resource,
                expires_at: None,
            },
            client_secret: client.secret,
            actor,
            verifier,
            redirect_uri,
            started: Instant::now(),
        };
        let mut flows = self.oauth_flows.lock().unwrap();
        flows.retain(|_, flow| flow.started.elapsed() < FLOW_LIFETIME);
        flows.insert(state, pending);
        Ok(authorize.to_string())
    }

    /// The sign-in a callback's `state` belongs to. It can be claimed once.
    pub fn claim_oauth(&self, state: &str) -> Option<Pending> {
        self.oauth_flows.lock().unwrap().remove(state).filter(|flow| flow.started.elapsed() < FLOW_LIFETIME)
    }

    /// Trade the code a sign-in came back with for tokens.
    pub async fn redeem_oauth(&self, pending: &Pending, code: &str) -> Result<Tokens> {
        let grant = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", pending.redirect_uri.as_str()),
            ("code_verifier", pending.verifier.as_str()),
        ];
        token_request(&self.http, &pending.spec.title, &pending.oauth, pending.client_secret.as_deref(), &grant).await
    }

    /// Save a finished sign-in: the tokens, and the connection they are for.
    pub async fn save_oauth(&self, pending: &Pending, tokens: &Tokens) -> Result<()> {
        let Spec { name, preset, title, url, headers, options } = &pending.spec;
        store::put_secret(&self.db, &self.secrets, &secret_key(name), &tokens.access).await?;
        match &tokens.refresh {
            Some(refresh) => store::put_secret(&self.db, &self.secrets, &refresh_key(name), refresh).await?,
            None => store::delete_secret(&self.db, &refresh_key(name)).await?,
        }
        match &pending.client_secret {
            Some(secret) => store::put_secret(&self.db, &self.secrets, &client_secret_key(name), secret).await?,
            None => store::delete_secret(&self.db, &client_secret_key(name)).await?,
        }
        let auth = Auth {
            header: Some("Authorization".into()),
            prefix: "Bearer ".into(),
            oauth: Some(OAuth { expires_at: tokens.expires_at, ..pending.oauth.clone() }),
        };
        sqlx::query(
            "INSERT INTO connections (name, preset, title, url, auth, headers, options, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
             ON CONFLICT (name) DO UPDATE SET title = excluded.title, url = excluded.url, auth = excluded.auth, headers = excluded.headers,
                 options = excluded.options, updated_by = excluded.updated_by, updated_at = now()",
        )
        .bind(name)
        .bind(preset)
        .bind(title)
        .bind(url)
        .bind(Json(&auth))
        .bind(Json(headers))
        .bind(options)
        .bind(pending.actor)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Get a new access token for a connection, unless another request just
    /// did. `refused` is the token a server turned down; without it, the
    /// token is only replaced if it is about to expire.
    pub async fn refresh_oauth(&self, name: &str, refused: Option<&str>) -> Result<()> {
        // One at a time: a refresh token may be single-use.
        let _guard = self.oauth_refresh.lock().await;
        let conn = by_name(&self.db, name).await?.ok_or_else(|| anyhow!("no connection named `{name}`"))?;
        let Some(oauth) = conn.auth.oauth.clone() else { return Ok(()) };
        let current = store::get_secret(&self.db, &self.secrets, &secret_key(name)).await?;
        let already = match refused {
            Some(token) => current.as_deref() != Some(token),
            None => !oauth.expiring(),
        };
        if already {
            return Ok(());
        }
        let again = format!("An admin can sign in to {} again under Settings → Integrations.", conn.title);
        let refresh = store::get_secret(&self.db, &self.secrets, &refresh_key(name)).await?.ok_or_else(|| anyhow!("{}'s sign-in has expired. {again}", conn.title))?;
        let client_secret = store::get_secret(&self.db, &self.secrets, &client_secret_key(name)).await?;
        let grant = [("grant_type", "refresh_token"), ("refresh_token", refresh.as_str())];
        let tokens = token_request(&self.http, &conn.title, &oauth, client_secret.as_deref(), &grant)
            .await
            .with_context(|| format!("{}'s sign-in could not be renewed. {again}", conn.title))?;
        store::put_secret(&self.db, &self.secrets, &secret_key(name), &tokens.access).await?;
        // A server that does not rotate refresh tokens sends none back.
        if let Some(next) = &tokens.refresh {
            store::put_secret(&self.db, &self.secrets, &refresh_key(name), next).await?;
        }
        let auth = Auth { oauth: Some(OAuth { expires_at: tokens.expires_at, ..oauth }), ..conn.auth.0.clone() };
        sqlx::query("UPDATE connections SET auth = $2 WHERE name = $1").bind(name).bind(Json(&auth)).execute(&self.db).await?;
        Ok(())
    }
}

impl Connection {
    /// Whether the access token should be replaced before it is used.
    pub fn token_expiring(&self) -> bool {
        self.auth.oauth.as_ref().is_some_and(OAuth::expiring)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_challenge_parameters() {
        let header = r#"Bearer realm="OAuth", resource_metadata="https://mcp.example/.well-known/oauth-protected-resource/mcp", scope="read write""#;
        assert_eq!(challenge_param(header, "resource_metadata").as_deref(), Some("https://mcp.example/.well-known/oauth-protected-resource/mcp"));
        assert_eq!(challenge_param(header, "scope").as_deref(), Some("read write"));
        assert_eq!(challenge_param("Bearer scope=files, error=\"x\"", "scope").as_deref(), Some("files"));
        assert_eq!(challenge_param(header, "error"), None);
        // `scope` inside another parameter's name is not the scope.
        assert_eq!(challenge_param(r#"Bearer max_scope="all""#, "scope"), None);
    }

    #[test]
    fn metadata_locations_follow_the_identifier() {
        let mcp = Url::parse("https://mcp.example/v1/mcp?x=1").unwrap();
        assert_eq!(well_known(&mcp, "oauth-protected-resource", true), "https://mcp.example/.well-known/oauth-protected-resource/v1/mcp");
        assert_eq!(well_known(&mcp, "oauth-protected-resource", false), "https://mcp.example/.well-known/oauth-protected-resource");
        let issuer = Url::parse("https://auth.example/tenant/").unwrap();
        assert_eq!(well_known(&issuer, "oauth-authorization-server", true), "https://auth.example/.well-known/oauth-authorization-server/tenant");
    }

    #[test]
    fn pkce_challenge_matches_the_rfc_example() {
        // RFC 7636, appendix B.
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_eq!(random(32).len(), 43);
    }
}
