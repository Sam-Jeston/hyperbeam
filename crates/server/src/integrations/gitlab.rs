//! GitLab credentials and REST access.
//!
//! One access token (personal, group or project) and the address of the
//! instance: gitlab.com unless the team runs its own.

use crate::app::App;
use crate::integrations::Host;
use anyhow::{Context, Result, bail};
use reqwest::Method;
use serde_json::Value;
use std::time::Duration;

pub const DEFAULT_URL: &str = "https://gitlab.com";

/// The instance's address from the integration's config, without a trailing slash.
pub fn base_url(config: &Value) -> String {
    config.get("url").and_then(Value::as_str).map(str::trim).filter(|url| !url.is_empty()).unwrap_or(DEFAULT_URL).trim_end_matches('/').to_string()
}

/// Check an instance address an admin typed, and tidy it.
pub fn clean_url(input: &str) -> Result<String> {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Ok(DEFAULT_URL.to_string());
    }
    let url = url::Url::parse(trimmed).context("write the GitLab address as a URL, like https://gitlab.example.com")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || url.query().is_some() || url.fragment().is_some() || !url.username().is_empty() {
        bail!("write the GitLab address as a URL, like https://gitlab.example.com");
    }
    Ok(trimmed.to_string())
}

/// A project is addressed by its URL-encoded path.
fn project(repo: &str) -> String {
    repo.replace('/', "%2F")
}

impl App {
    async fn gitlab_request(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let (config, token) = self.host_credential(Host::Gitlab).await?;
        let mut request = self
            .http
            .request(method, format!("{}/api/v4{path}", base_url(&config)))
            .header("private-token", token)
            .header("accept", "application/json")
            .timeout(Duration::from_secs(30));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.context("could not reach GitLab")?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("GitLab returned {status}: {}", error_message(&text));
        }
        if text.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).context("GitLab sent an unreadable reply")
    }

    /// Call the GitLab REST API on behalf of a project. `path` continues
    /// `/projects/:id`.
    pub async fn gitlab_api(&self, method: Method, repo: &str, path: &str, body: Option<Value>) -> Result<Value> {
        self.gitlab_request(method, &format!("/projects/{}{path}", project(repo)), body).await
    }

    pub(super) async fn gitlab_check(&self) -> Result<String> {
        let user = self.gitlab_request(Method::GET, "/user", None).await?;
        Ok(format!("Authenticated to GitLab as {}", user["username"].as_str().unwrap_or("unknown")))
    }
}

/// The useful part of a GitLab error body: `message` or `error`, either of
/// which may be text or a structure of field errors.
pub fn error_message(body: &str) -> String {
    let clipped = || body.chars().take(300).collect::<String>();
    let Ok(value) = serde_json::from_str::<Value>(body) else { return clipped() };
    let mut parts = Vec::new();
    for key in ["message", "error", "error_description"] {
        match value.get(key) {
            Some(Value::String(text)) => parts.push(text.clone()),
            Some(Value::Null) | None => {}
            Some(other) => parts.push(other.to_string()),
        }
    }
    if parts.is_empty() { clipped() } else { parts.join("; ") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_error_messages() {
        assert_eq!(error_message(r#"{"message":"404 Project Not Found"}"#), "404 Project Not Found");
        assert_eq!(error_message(r#"{"message":{"title":["can't be blank"]}}"#), r#"{"title":["can't be blank"]}"#);
        assert_eq!(error_message(r#"{"error":"invalid_token","error_description":"Token was revoked"}"#), "invalid_token; Token was revoked");
        assert_eq!(error_message("plain"), "plain");
    }

    #[test]
    fn instance_addresses() {
        assert_eq!(base_url(&json!({})), "https://gitlab.com");
        assert_eq!(base_url(&json!({ "url": "https://git.acme.dev/gitlab/" })), "https://git.acme.dev/gitlab");
        assert_eq!(clean_url("").unwrap(), "https://gitlab.com");
        assert_eq!(clean_url(" https://git.acme.dev/ ").unwrap(), "https://git.acme.dev");
        for bad in ["git.acme.dev", "ssh://git.acme.dev", "https://user@git.acme.dev", "https://git.acme.dev?x=1"] {
            assert!(clean_url(bad).is_err(), "{bad}");
        }
        assert_eq!(project("acme/platform/api"), "acme%2Fplatform%2Fapi");
    }
}
