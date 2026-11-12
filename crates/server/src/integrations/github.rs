//! GitHub credentials and REST access.
//!
//! Two modes: a personal access token (simple, good for one person) or a
//! GitHub App (recommended for teams: short-lived tokens minted per
//! repository with only the permissions a request needs).

use crate::app::App;
pub use crate::integrations::Access;
use crate::integrations::Host;
use anyhow::{Context, Result, anyhow, bail};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::Method;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub struct GithubTokens {
    cache: Mutex<HashMap<String, (String, Instant)>>,
}

impl GithubTokens {
    pub fn clear(&self) {
        self.cache.lock().unwrap().clear();
    }
}

fn app_jwt(app_id: &str, private_key_pem: &str) -> Result<String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let claims = json!({ "iat": now - 60, "exp": now + 9 * 60, "iss": app_id });
    let key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
        .context("the GitHub App private key is not a valid RSA PEM")?;
    Ok(jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &key)?)
}

impl App {
    /// A token able to act on `repo` (`owner/name`) with the given access.
    pub async fn github_token(&self, repo: &str, access: Access) -> Result<String> {
        let (config, secret) = self.host_credential(Host::Github).await?;
        if config.get("mode").and_then(Value::as_str) != Some("app") {
            return Ok(secret);
        }

        let cache_key = format!("{}|{:?}", repo.to_lowercase(), access);
        if let Some((token, expires)) = self.github_tokens.cache.lock().unwrap().get(&cache_key)
            && *expires > Instant::now() {
                return Ok(token.clone());
            }
        let app_id = match config.get("app_id") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => bail!("the GitHub App integration has no app_id"),
        };
        let jwt = app_jwt(&app_id, &secret)?;
        let api = self.cfg.github_api_url.trim_end_matches('/');
        let gh = |method: Method, url: String| {
            self.http
                .request(method, url)
                .bearer_auth(&jwt)
                .header("accept", "application/vnd.github+json")
                .header("x-github-api-version", "2022-11-28")
        };

        let installation = gh(Method::GET, format!("{api}/repos/{repo}/installation")).send().await?;
        if !installation.status().is_success() {
            bail!("the Hyperbeam GitHub App is not installed on {repo} (GitHub said {})", installation.status());
        }
        let installation: Value = installation.json().await?;
        let installation_id = installation.get("id").and_then(Value::as_i64).ok_or_else(|| anyhow!("unexpected reply from GitHub"))?;

        let level = if access == Access::Write { "write" } else { "read" };
        let name = repo.rsplit('/').next().unwrap_or(repo);
        let minted = gh(Method::POST, format!("{api}/app/installations/{installation_id}/access_tokens"))
            .json(&json!({
                "repositories": [name],
                "permissions": { "contents": level, "pull_requests": level, "issues": level, "metadata": "read" },
            }))
            .send()
            .await?;
        if !minted.status().is_success() {
            let status = minted.status();
            let body = minted.text().await.unwrap_or_default();
            bail!("GitHub would not issue a {level} token for {repo} ({status}): {}", error_message(&body));
        }
        let minted: Value = minted.json().await?;
        let token = minted.get("token").and_then(Value::as_str).ok_or_else(|| anyhow!("GitHub returned no token"))?.to_string();
        // Installation tokens last an hour; reuse for well under that.
        self.github_tokens
            .cache
            .lock()
            .unwrap()
            .insert(cache_key, (token.clone(), Instant::now() + Duration::from_secs(40 * 60)));
        Ok(token)
    }

    /// Call the GitHub REST API on behalf of a repository.
    pub async fn github_api(
        &self,
        method: Method,
        repo: &str,
        path: &str,
        access: Access,
        body: Option<Value>,
    ) -> Result<Value> {
        let token = self.github_token(repo, access).await?;
        let url = format!("{}/repos/{repo}{path}", self.cfg.github_api_url.trim_end_matches('/'));
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(token)
            .header("accept", "application/vnd.github+json")
            .header("x-github-api-version", "2022-11-28")
            .timeout(Duration::from_secs(30));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.context("could not reach GitHub")?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("GitHub returned {status}: {}", error_message(&text));
        }
        if text.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).context("GitHub sent an unreadable reply")
    }

    pub(super) async fn github_check(&self) -> Result<String> {
        let (config, secret) = self.host_credential(Host::Github).await?;
        if config["mode"] == "app" {
            // Minting a token for a registered repository exercises the whole chain.
            let repo: Option<(String,)> =
                sqlx::query_as("SELECT full_name FROM repos WHERE host = 'github' ORDER BY created_at LIMIT 1").fetch_optional(&self.db).await?;
            return match repo {
                Some((repo,)) => {
                    self.github_token(&repo, Access::Read).await?;
                    Ok(format!("GitHub App can read {repo}"))
                }
                None => Ok("Saved. Register a repository to verify the App is installed on it.".to_string()),
            };
        }
        let api = self.cfg.github_api_url.trim_end_matches('/');
        let resp = self.http.get(format!("{api}/user")).bearer_auth(&secret).header("accept", "application/vnd.github+json").send().await?;
        if !resp.status().is_success() {
            bail!("GitHub rejected the token ({})", resp.status());
        }
        let user: Value = resp.json().await?;
        Ok(format!("Authenticated to GitHub as {}", user["login"].as_str().unwrap_or("unknown")))
    }
}

/// The useful part of a GitHub error body.
pub fn error_message(body: &str) -> String {
    match serde_json::from_str::<Value>(body) {
        Ok(v) => {
            let mut message = v.get("message").and_then(Value::as_str).unwrap_or("").to_string();
            if let Some(errors) = v.get("errors").and_then(Value::as_array) {
                for err in errors {
                    let detail = err.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| err.to_string());
                    message.push_str(&format!("; {detail}"));
                }
            }
            if message.is_empty() { body.chars().take(300).collect() } else { message }
        }
        Err(_) => body.chars().take(300).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_error_messages() {
        let body = r#"{"message":"Validation Failed","errors":[{"message":"A pull request already exists"}]}"#;
        assert_eq!(error_message(body), "Validation Failed; A pull request already exists");
        assert_eq!(error_message("plain"), "plain");
    }
}
