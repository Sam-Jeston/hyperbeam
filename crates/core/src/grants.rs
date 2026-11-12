//! The permission model.
//!
//! A [`Grant`] is one capability an agent may exercise against an external
//! system, written `system:level`. People hold grants; an agent session
//! receives the *intersection* of what its launching user holds and what the
//! session asked for. Agents never hold more than the person who started them.
//!
//! Code hosts (GitHub and GitLab) are built in and have four levels. Every
//! other system (Notion, and each MCP connection an admin adds) has two:
//! `read` and `write`. Which systems exist is the control plane's business;
//! this module only checks that a grant is well formed.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Level {
    Read,
    Write,
    /// Code hosts only. Open pull (or merge) requests.
    OpenPr,
    /// Code hosts only. Merge them, and push to the default branch.
    MergePr,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Read => "read",
            Level::Write => "write",
            Level::OpenPr => "open_pr",
            Level::MergePr => "merge_pr",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Grant {
    system: Cow<'static, str>,
    level: Level,
}

/// The system name GitHub's grants use.
pub const GITHUB: &str = "github";
/// The system name GitLab's grants use.
pub const GITLAB: &str = "gitlab";
/// Systems that host repositories. Their grants have all four levels.
pub const CODE_HOSTS: [&str; 2] = [GITHUB, GITLAB];
/// The system name for reading the public web. It has one level, `read`.
pub const WEB: &str = "web";

impl Grant {
    /// Clone repositories, read issues, pull requests and code.
    pub const GITHUB_READ: Grant = Grant::builtin(GITHUB, Level::Read);
    /// Push branches, comment, create and edit issues.
    pub const GITHUB_WRITE: Grant = Grant::builtin(GITHUB, Level::Write);
    /// Open pull requests.
    pub const GITHUB_OPEN_PR: Grant = Grant::builtin(GITHUB, Level::OpenPr);
    /// Merge pull requests and push to protected (default) branches.
    pub const GITHUB_MERGE_PR: Grant = Grant::builtin(GITHUB, Level::MergePr);

    /// Read public web pages through the gateway's `web_fetch`.
    pub const WEB_READ: Grant = Grant::builtin(WEB, Level::Read);

    pub const GITHUB_ALL: [Grant; 4] = Grant::all_on_host(GITHUB);
    pub const GITLAB_ALL: [Grant; 4] = Grant::all_on_host(GITLAB);

    const fn builtin(system: &'static str, level: Level) -> Grant {
        Grant { system: Cow::Borrowed(system), level }
    }

    /// A grant on a code host. `host` must be one of [`CODE_HOSTS`].
    pub const fn on_host(host: &'static str, level: Level) -> Grant {
        Grant::builtin(host, level)
    }

    /// Every level a code host has, weakest first.
    pub const fn all_on_host(host: &'static str) -> [Grant; 4] {
        [
            Grant::builtin(host, Level::Read),
            Grant::builtin(host, Level::Write),
            Grant::builtin(host, Level::OpenPr),
            Grant::builtin(host, Level::MergePr),
        ]
    }

    /// `read` or `write` on a system that is not a code host. The name must be valid.
    pub fn new(system: &str, level: Level) -> Result<Grant, UnknownGrant> {
        format!("{system}:{}", level.as_str()).parse()
    }

    pub fn read(system: &str) -> Result<Grant, UnknownGrant> {
        Grant::new(system, Level::Read)
    }

    pub fn write(system: &str) -> Result<Grant, UnknownGrant> {
        Grant::new(system, Level::Write)
    }

    pub fn system(&self) -> &str {
        &self.system
    }

    pub fn level(&self) -> Level {
        self.level
    }

    /// What this grant lets an agent do. Connections describe their own
    /// grants more specifically; this is the fallback.
    pub fn label(&self) -> &'static str {
        match (self.system(), self.level) {
            (GITHUB, Level::Read) => "Read repositories, issues and pull requests",
            (GITHUB, Level::Write) => "Push branches, comment, create issues",
            (GITHUB, Level::OpenPr) => "Open pull requests",
            (GITHUB, Level::MergePr) => "Merge pull requests",
            (GITLAB, Level::Read) => "Read repositories, issues and merge requests",
            (GITLAB, Level::Write) => "Push branches, comment, create issues",
            (GITLAB, Level::OpenPr) => "Open merge requests",
            (GITLAB, Level::MergePr) => "Merge merge requests",
            (WEB, Level::Read) => "Read public web pages",
            (_, Level::Read) => "Use read tools",
            _ => "Use write tools",
        }
    }

    /// Grants that are meaningless without this one. Holding `github:write`
    /// without `github:read` makes no sense, so the stronger grant implies the
    /// weaker.
    fn implies(&self) -> Option<Grant> {
        (self.level != Level::Read).then(|| Grant { system: self.system.clone(), level: Level::Read })
    }
}

/// A system name: lowercase letters, digits and dashes, starting with a
/// letter. Underscores are excluded so `system_tool` names split cleanly.
pub fn valid_system(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.ends_with('-')
}

impl fmt::Display for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.system, self.level.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown grant `{0}`")]
pub struct UnknownGrant(pub String);

impl FromStr for Grant {
    type Err = UnknownGrant;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let unknown = || UnknownGrant(s.to_string());
        let (system, level) = s.split_once(':').ok_or_else(unknown)?;
        if let Some(host) = CODE_HOSTS.into_iter().find(|host| *host == system) {
            return Grant::all_on_host(host).into_iter().find(|g| g.level.as_str() == level).ok_or_else(unknown);
        }
        let level = match level {
            "read" => Level::Read,
            "write" => Level::Write,
            _ => return Err(unknown()),
        };
        if !valid_system(system) || (system == WEB && level != Level::Read) {
            return Err(unknown());
        }
        Ok(Grant { system: Cow::Owned(system.to_string()), level })
    }
}

impl Serialize for Grant {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Grant {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// A normalised set of grants (implied grants are always present).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<Grant>", into = "Vec<Grant>")]
pub struct GrantSet(BTreeSet<Grant>);

impl GrantSet {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn has(&self, g: &Grant) -> bool {
        self.0.contains(g)
    }

    /// Holds `system:level`, for systems named at runtime.
    pub fn allows(&self, system: &str, level: Level) -> bool {
        self.0.iter().any(|g| g.system() == system && g.level == level)
    }

    pub fn insert(&mut self, g: Grant) {
        if let Some(implied) = g.implies() {
            self.0.insert(implied);
        }
        self.0.insert(g);
    }

    /// Drop every grant on `system`.
    pub fn remove_system(&mut self, system: &str) {
        self.0.retain(|g| g.system() != system);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Grant> + '_ {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// What an agent may do: only what both sides allow.
    pub fn intersect(&self, other: &GrantSet) -> GrantSet {
        // Intersection of two normalised sets is itself normalised: if a
        // grant is in both, so are its implications.
        GrantSet(self.0.intersection(&other.0).cloned().collect())
    }

    pub fn is_subset(&self, other: &GrantSet) -> bool {
        self.0.is_subset(&other.0)
    }

    pub fn parse_many<I, S>(items: I) -> Result<Self, UnknownGrant>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut set = GrantSet::empty();
        for s in items {
            set.insert(s.as_ref().parse()?);
        }
        Ok(set)
    }

    pub fn to_strings(&self) -> Vec<String> {
        self.0.iter().map(Grant::to_string).collect()
    }
}

impl FromIterator<Grant> for GrantSet {
    fn from_iter<T: IntoIterator<Item = Grant>>(iter: T) -> Self {
        let mut set = GrantSet::empty();
        for g in iter {
            set.insert(g);
        }
        set
    }
}

impl From<Vec<Grant>> for GrantSet {
    fn from(v: Vec<Grant>) -> Self {
        v.into_iter().collect()
    }
}

impl From<GrantSet> for Vec<Grant> {
    fn from(s: GrantSet) -> Self {
        s.0.into_iter().collect()
    }
}

/// Which repositories a principal may touch. `None` means every repository
/// registered with the platform.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepoScope(pub Option<BTreeSet<String>>);

impl RepoScope {
    pub fn all() -> Self {
        RepoScope(None)
    }

    pub fn only<I: IntoIterator<Item = S>, S: Into<String>>(repos: I) -> Self {
        RepoScope(Some(repos.into_iter().map(|r| r.into().to_lowercase()).collect()))
    }

    pub fn allows(&self, repo: &str) -> bool {
        match &self.0 {
            None => true,
            Some(set) => set.contains(&repo.to_lowercase()),
        }
    }
}

/// Team role. Roles govern the platform itself (who may invite, configure
/// integrations, edit skills); grants govern what agents may do outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Member,
    Admin,
    Owner,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Member => "member",
            Role::Admin => "admin",
            Role::Owner => "owner",
        }
    }

    pub fn is_admin(self) -> bool {
        self >= Role::Admin
    }
}

impl FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "member" => Ok(Role::Member),
            "admin" => Ok(Role::Admin),
            "owner" => Ok(Role::Owner),
            other => Err(format!("unknown role `{other}`")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stronger_grants_imply_read() {
        let set = GrantSet::parse_many(["github:merge_pr", "linear:write"]).unwrap();
        assert!(set.has(&Grant::GITHUB_READ));
        assert!(set.has(&Grant::read("linear").unwrap()));
        assert!(!set.has(&Grant::GITHUB_WRITE));
        assert!(!set.allows("notion", Level::Read));
    }

    #[test]
    fn agent_never_exceeds_its_user() {
        let user = GrantSet::parse_many(["github:read", "github:open_pr"]).unwrap();
        let requested = GrantSet::parse_many(["github:merge_pr", "github:write", "github:open_pr", "sentry:write"]).unwrap();
        let effective = user.intersect(&requested);
        assert_eq!(effective, user);
        assert!(!effective.has(&Grant::GITHUB_MERGE_PR));
        assert!(!effective.allows("sentry", Level::Read));
    }

    #[test]
    fn round_trips_as_strings() {
        let set = GrantSet::parse_many(["notion:write"]).unwrap();
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(json, r#"["notion:read","notion:write"]"#);
        let back: GrantSet = serde_json::from_str(&json).unwrap();
        assert_eq!(back, set);
        assert!(serde_json::from_str::<GrantSet>(r#"["github:admin"]"#).is_err());
    }

    #[test]
    fn only_code_hosts_have_extra_levels_and_names_are_checked() {
        assert!("posthog:read".parse::<Grant>().is_ok());
        assert_eq!("gitlab:merge_pr".parse::<Grant>().unwrap(), Grant::on_host(GITLAB, Level::MergePr));
        assert_eq!("gitlab:open_pr".parse::<Grant>().unwrap().label(), "Open merge requests");
        assert!("gitlab:admin".parse::<Grant>().is_err());
        assert!("my-mcp2:write".parse::<Grant>().is_ok());
        assert_eq!("web:read".parse::<Grant>().unwrap(), Grant::WEB_READ);
        for bad in ["web:write", "sentry:merge_pr", "Sentry:read", "my_mcp:read", "-x:read", "x-:read", "nocolon", ":read", "a:"] {
            assert!(bad.parse::<Grant>().is_err(), "{bad}");
        }
    }

    #[test]
    fn removing_a_system_removes_all_its_levels() {
        let mut set = GrantSet::parse_many(["github:open_pr", "sentry:write"]).unwrap();
        set.remove_system("sentry");
        assert_eq!(set.to_strings(), ["github:read", "github:open_pr"]);
    }

    #[test]
    fn repo_scope_is_case_insensitive() {
        let scope = RepoScope::only(["Acme/API"]);
        assert!(scope.allows("acme/api"));
        assert!(!scope.allows("acme/web"));
        assert!(RepoScope::all().allows("anything/at-all"));
    }

    #[test]
    fn roles_order() {
        assert!(Role::Owner.is_admin());
        assert!(Role::Admin.is_admin());
        assert!(!Role::Member.is_admin());
    }
}
