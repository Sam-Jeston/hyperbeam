//! The permission model.
//!
//! A [`Grant`] is one capability an agent may exercise against an external
//! system. People hold grants; an agent session receives the *intersection* of
//! what its launching user holds and what the session asked for. Agents never
//! hold more than the person who started them.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Grant {
    /// Clone repositories, read issues, pull requests and code.
    GithubRead,
    /// Push branches, comment, create and edit issues.
    GithubWrite,
    /// Open pull requests.
    GithubOpenPr,
    /// Merge pull requests and push to protected (default) branches.
    GithubMergePr,
    LinearRead,
    LinearWrite,
    NotionRead,
    NotionWrite,
}

impl Grant {
    pub const ALL: [Grant; 8] = [
        Grant::GithubRead,
        Grant::GithubWrite,
        Grant::GithubOpenPr,
        Grant::GithubMergePr,
        Grant::LinearRead,
        Grant::LinearWrite,
        Grant::NotionRead,
        Grant::NotionWrite,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Grant::GithubRead => "github:read",
            Grant::GithubWrite => "github:write",
            Grant::GithubOpenPr => "github:open_pr",
            Grant::GithubMergePr => "github:merge_pr",
            Grant::LinearRead => "linear:read",
            Grant::LinearWrite => "linear:write",
            Grant::NotionRead => "notion:read",
            Grant::NotionWrite => "notion:write",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Grant::GithubRead => "Read repositories, issues and pull requests",
            Grant::GithubWrite => "Push branches, comment, create issues",
            Grant::GithubOpenPr => "Open pull requests",
            Grant::GithubMergePr => "Merge pull requests",
            Grant::LinearRead => "Read issues and projects",
            Grant::LinearWrite => "Create and update issues, comment",
            Grant::NotionRead => "Read pages and databases",
            Grant::NotionWrite => "Create and edit pages",
        }
    }

    pub fn system(self) -> &'static str {
        self.as_str().split(':').next().unwrap_or_default()
    }

    /// Grants that are meaningless without this one. Holding `github:write`
    /// without `github:read` makes no sense, so the stronger grant implies the
    /// weaker.
    fn implies(self) -> &'static [Grant] {
        match self {
            Grant::GithubWrite | Grant::GithubOpenPr | Grant::GithubMergePr => &[Grant::GithubRead],
            Grant::LinearWrite => &[Grant::LinearRead],
            Grant::NotionWrite => &[Grant::NotionRead],
            _ => &[],
        }
    }
}

impl fmt::Display for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown grant `{0}`")]
pub struct UnknownGrant(pub String);

impl FromStr for Grant {
    type Err = UnknownGrant;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Grant::ALL
            .into_iter()
            .find(|g| g.as_str() == s)
            .ok_or_else(|| UnknownGrant(s.to_string()))
    }
}

impl Serialize for Grant {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
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

    pub fn all() -> Self {
        Grant::ALL.into_iter().collect()
    }

    pub fn has(&self, g: Grant) -> bool {
        self.0.contains(&g)
    }

    pub fn insert(&mut self, g: Grant) {
        self.0.insert(g);
        for implied in g.implies() {
            self.0.insert(*implied);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = Grant> + '_ {
        self.0.iter().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// What an agent may do: only what both sides allow.
    pub fn intersect(&self, other: &GrantSet) -> GrantSet {
        // Intersection of two normalised sets is itself normalised: if a
        // grant is in both, so are its implications.
        GrantSet(self.0.intersection(&other.0).copied().collect())
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
        self.0.iter().map(|g| g.as_str().to_string()).collect()
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
        assert!(set.has(Grant::GithubRead));
        assert!(set.has(Grant::LinearRead));
        assert!(!set.has(Grant::GithubWrite));
        assert!(!set.has(Grant::NotionRead));
    }

    #[test]
    fn agent_never_exceeds_its_user() {
        let user = GrantSet::parse_many(["github:read", "github:open_pr"]).unwrap();
        let requested = GrantSet::all();
        let effective = user.intersect(&requested);
        assert_eq!(effective, user);
        assert!(!effective.has(Grant::GithubMergePr));
        assert!(!effective.has(Grant::GithubWrite));
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
