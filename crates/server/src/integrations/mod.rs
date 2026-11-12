//! Credentials for the external systems agents reach through the gateway.
//! They are held here, in the control plane, and never enter a sandbox.
//!
//! The built-in integrations are code hosts: the places repositories live.
//! [`Host`] is what the rest of the server works with; each host's own
//! module knows how to authenticate to it and speak its API.

pub mod github;
pub mod gitlab;

use crate::app::App;
use crate::store;
use anyhow::{Result, anyhow};
use hyperbeam_core::{Grant, Level, grants};
use serde_json::Value;

/// Built-in integrations. Everything else is an MCP connection.
pub const KINDS: [&str; 2] = ["github", "gitlab"];

pub fn secret_key(kind: &str) -> String {
    format!("integration/{kind}")
}

/// How much a credential must be able to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

/// A code host: somewhere repositories live, with issues and reviewed
/// changes around them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    Github,
    Gitlab,
}

impl Host {
    pub const ALL: [Host; 2] = [Host::Github, Host::Gitlab];

    /// The integration kind, which is also the system its grants and tools
    /// are named after.
    pub fn kind(self) -> &'static str {
        match self {
            Host::Github => grants::GITHUB,
            Host::Gitlab => grants::GITLAB,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Host::Github => "GitHub",
            Host::Gitlab => "GitLab",
        }
    }

    pub fn parse(kind: &str) -> Option<Host> {
        Host::ALL.into_iter().find(|host| host.kind() == kind)
    }

    pub fn grant(self, level: Level) -> Grant {
        Grant::on_host(self.kind(), level)
    }

    /// What the host calls a change put up for review.
    pub fn change_request(self) -> &'static str {
        match self {
            Host::Github => "pull request",
            Host::Gitlab => "merge request",
        }
    }

    /// Whether `name` could be a repository on this host. GitLab nests
    /// groups; GitHub is always `owner/name`.
    pub fn accepts_repo(self, name: &str) -> bool {
        hyperbeam_core::workflow::valid_repo(name) && (self == Host::Gitlab || name.split('/').count() == 2)
    }
}

/// Where the git proxy forwards a repository's traffic, and as whom.
pub struct GitUpstream {
    /// The repository's smart-HTTP base, without a trailing slash.
    pub url: String,
    pub username: &'static str,
    pub token: String,
}

/// What a host says about a repository when it is registered.
pub struct RepoInfo {
    pub full_name: String,
    pub default_branch: Option<String>,
    pub description: String,
}

impl App {
    pub async fn integration_config(&self, kind: &str) -> Result<Option<Value>> {
        let row: Option<(Value,)> =
            sqlx::query_as("SELECT config FROM integrations WHERE kind = $1").bind(kind).fetch_optional(&self.db).await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn integration_secret(&self, kind: &str) -> Result<Option<String>> {
        if self.integration_config(kind).await?.is_none() {
            return Ok(None);
        }
        store::get_secret(&self.db, &self.secrets, &secret_key(kind)).await
    }

    /// The config and credential of a connected host.
    async fn host_credential(&self, host: Host) -> Result<(Value, String)> {
        let title = host.title();
        let config = self
            .integration_config(host.kind())
            .await?
            .ok_or_else(|| anyhow!("{title} is not connected. An admin can connect it under Settings → Integrations."))?;
        let secret = self.integration_secret(host.kind()).await?.ok_or_else(|| anyhow!("the {title} credential is missing"))?;
        Ok((config, secret))
    }

    /// The code hosts with a credential, in a fixed order.
    pub async fn connected_hosts(&self) -> Result<Vec<Host>> {
        let kinds: Vec<(String,)> = sqlx::query_as("SELECT kind FROM integrations").fetch_all(&self.db).await?;
        Ok(Host::ALL.into_iter().filter(|host| kinds.iter().any(|(kind,)| kind == host.kind())).collect())
    }

    /// The host a registered repository is on. A repository unregistered
    /// while a session still holds it dates from before there was a choice.
    pub async fn repo_host(&self, repo: &str) -> Result<Host> {
        Ok(store::repo_by_name(&self.db, repo).await?.and_then(|r| Host::parse(&r.host)).unwrap_or(Host::Github))
    }

    /// Where the host lives on the web. A checkout's `origin` is under it.
    pub async fn host_web_url(&self, host: Host) -> Result<String> {
        Ok(match host {
            Host::Github => "https://github.com".to_string(),
            Host::Gitlab => gitlab::base_url(&self.integration_config(host.kind()).await?.unwrap_or_default()),
        })
    }

    /// Where to send `repo`'s git traffic, with a credential for it.
    pub async fn git_upstream(&self, host: Host, repo: &str, access: Access) -> Result<GitUpstream> {
        Ok(match host {
            Host::Github => GitUpstream {
                url: format!("{}/{repo}.git", self.cfg.github_git_url.trim_end_matches('/')),
                username: "x-access-token",
                token: self.github_token(repo, access).await?,
            },
            Host::Gitlab => {
                let (config, token) = self.host_credential(host).await?;
                GitUpstream { url: format!("{}/{repo}.git", gitlab::base_url(&config)), username: "oauth2", token }
            }
        })
    }

    /// Confirm the credential can read `name` and learn how the host writes it.
    pub async fn host_repo_info(&self, host: Host, name: &str) -> Result<RepoInfo> {
        let text = |value: &Value| value.as_str().map(str::to_string);
        Ok(match host {
            Host::Github => {
                let info = self.github_api(reqwest::Method::GET, name, "", Access::Read, None).await?;
                RepoInfo {
                    full_name: text(&info["full_name"]).unwrap_or_else(|| name.to_string()),
                    default_branch: text(&info["default_branch"]),
                    description: text(&info["description"]).unwrap_or_default(),
                }
            }
            Host::Gitlab => {
                let info = self.gitlab_api(reqwest::Method::GET, name, "", None).await?;
                RepoInfo {
                    full_name: text(&info["path_with_namespace"]).unwrap_or_else(|| name.to_string()),
                    default_branch: text(&info["default_branch"]),
                    description: text(&info["description"]).unwrap_or_default(),
                }
            }
        })
    }

    /// Make one cheap authenticated call to confirm the credential works.
    pub async fn host_check(&self, host: Host) -> Result<String> {
        match host {
            Host::Github => self.github_check().await,
            Host::Gitlab => self.gitlab_check().await,
        }
    }
}
