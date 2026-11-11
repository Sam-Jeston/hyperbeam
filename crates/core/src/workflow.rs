//! Workflows: reusable, parameterised agent launches that may span several
//! repositories. Authored as YAML.

use crate::grants::GrantSet;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionRole {
    /// One agent doing one piece of work.
    #[default]
    Worker,
    /// An agent that plans, delegates to worker agents and tracks them.
    Conductor,
}

impl SessionRole {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionRole::Worker => "worker",
            SessionRole::Conductor => "conductor",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "worker" => Some(SessionRole::Worker),
            "conductor" => Some(SessionRole::Conductor),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub role: SessionRole,
    /// Harness profile name. Falls back to the team default when absent.
    #[serde(default)]
    pub harness: Option<String>,
    /// Model for this workflow's agents. Falls back to the harness default.
    #[serde(default)]
    pub model: Option<String>,
    /// `owner/name` repositories checked out into the workspace.
    #[serde(default)]
    pub repos: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    /// The most this workflow's agents may do. Still intersected with the
    /// launching user's own grants.
    #[serde(default)]
    pub grants: GrantSet,
    #[serde(default)]
    pub inputs: Vec<WorkflowInput>,
    /// Prompt template. `{{name}}` is replaced by the input of that name.
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum WorkflowError {
    #[error("invalid workflow YAML: {0}")]
    Yaml(String),
    #[error("workflow name must be lowercase letters, digits and dashes")]
    BadName,
    #[error("repository `{0}` must be written as owner/name")]
    BadRepo(String),
    #[error("prompt references `{{{{{0}}}}}` but no input of that name is declared")]
    UndeclaredInput(String),
    #[error("input `{0}` is declared twice")]
    DuplicateInput(String),
    #[error("missing required input `{0}`")]
    MissingInput(String),
    #[error("unknown input `{0}`")]
    UnknownInput(String),
}

pub fn valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

pub fn valid_repo(s: &str) -> bool {
    let ok = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    match s.split_once('/') {
        Some((owner, name)) => ok(owner) && ok(name),
        None => false,
    }
}

impl WorkflowSpec {
    pub fn from_yaml(yaml: &str) -> Result<Self, WorkflowError> {
        let spec: WorkflowSpec =
            serde_yaml_ng::from_str(yaml).map_err(|e| WorkflowError::Yaml(e.to_string()))?;
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> Result<(), WorkflowError> {
        if !valid_slug(&self.name) {
            return Err(WorkflowError::BadName);
        }
        for repo in &self.repos {
            if !valid_repo(repo) {
                return Err(WorkflowError::BadRepo(repo.clone()));
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for input in &self.inputs {
            if !seen.insert(input.name.as_str()) {
                return Err(WorkflowError::DuplicateInput(input.name.clone()));
            }
        }
        for name in placeholders(&self.prompt) {
            if !seen.contains(name.as_str()) {
                return Err(WorkflowError::UndeclaredInput(name));
            }
        }
        Ok(())
    }

    /// Fill the prompt template.
    pub fn render(&self, values: &BTreeMap<String, String>) -> Result<String, WorkflowError> {
        for key in values.keys() {
            if !self.inputs.iter().any(|i| &i.name == key) {
                return Err(WorkflowError::UnknownInput(key.clone()));
            }
        }
        let mut resolved: BTreeMap<&str, String> = BTreeMap::new();
        for input in &self.inputs {
            let value = values
                .get(&input.name)
                .filter(|v| !v.trim().is_empty())
                .cloned()
                .or_else(|| input.default.clone());
            let value = match value {
                Some(v) => v,
                None if input.required => return Err(WorkflowError::MissingInput(input.name.clone())),
                None => String::new(),
            };
            resolved.insert(&input.name, value);
        }
        // Single pass, so braces inside a substituted value are never expanded.
        let mut out = String::with_capacity(self.prompt.len());
        let mut rest = self.prompt.as_str();
        while let Some(start) = rest.find("{{") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else { break };
            out.push_str(&rest[..start]);
            match resolved.get(after[..end].trim()) {
                Some(value) => out.push_str(value),
                None => out.push_str(&rest[start..start + 2 + end + 2]),
            }
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        Ok(out)
    }
}

/// Names inside `{{ }}` in a template, in order of appearance.
fn placeholders(template: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim();
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            names.push(name.to_string());
        }
        rest = &after[end + 2..];
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grants::Grant;

    const YAML: &str = r#"
name: ship-feature
description: Build a feature across the API and the web app
role: conductor
harness: claude-code
repos: [acme/api, acme/web]
skills: [house-style]
grants: [github:write, github:open_pr, linear:read]
inputs:
  - name: feature
    description: What to build
  - name: ticket
    required: false
prompt: |
  Build {{feature}}. Ticket: {{ ticket }}.
"#;

    #[test]
    fn parses_and_renders() {
        let wf = WorkflowSpec::from_yaml(YAML).unwrap();
        assert_eq!(wf.role, SessionRole::Conductor);
        assert!(wf.grants.has(Grant::GithubRead));
        let mut values = BTreeMap::new();
        values.insert("feature".to_string(), "dark mode".to_string());
        assert_eq!(wf.render(&values).unwrap().trim(), "Build dark mode. Ticket: .");
    }

    #[test]
    fn substituted_values_are_not_re_expanded() {
        let wf = WorkflowSpec::from_yaml(YAML).unwrap();
        let mut values = BTreeMap::new();
        values.insert("feature".to_string(), "{{ticket}}".to_string());
        values.insert("ticket".to_string(), "ENG-1".to_string());
        let rendered = wf.render(&values).unwrap();
        assert_eq!(rendered.trim(), "Build {{ticket}}. Ticket: ENG-1.");
    }

    #[test]
    fn rejects_missing_and_unknown_inputs() {
        let wf = WorkflowSpec::from_yaml(YAML).unwrap();
        assert_eq!(wf.render(&BTreeMap::new()), Err(WorkflowError::MissingInput("feature".into())));
        let mut values = BTreeMap::new();
        values.insert("feature".to_string(), "x".to_string());
        values.insert("nope".to_string(), "y".to_string());
        assert_eq!(wf.render(&values), Err(WorkflowError::UnknownInput("nope".into())));
    }

    #[test]
    fn rejects_undeclared_placeholder_and_bad_repo() {
        let bad = "name: a\nprompt: 'do {{thing}}'\n";
        assert_eq!(WorkflowSpec::from_yaml(bad), Err(WorkflowError::UndeclaredInput("thing".into())));
        let bad = "name: a\nrepos: [justname]\nprompt: hi\n";
        assert_eq!(WorkflowSpec::from_yaml(bad), Err(WorkflowError::BadRepo("justname".into())));
        let bad = "name: Bad Name\nprompt: hi\n";
        assert_eq!(WorkflowSpec::from_yaml(bad), Err(WorkflowError::BadName));
        assert!(matches!(
            WorkflowSpec::from_yaml("name: a\nprompt: hi\ngrants: [github:root]\n"),
            Err(WorkflowError::Yaml(_))
        ));
    }

    #[test]
    fn repo_names() {
        assert!(valid_repo("acme/api.v2"));
        assert!(!valid_repo("acme/../etc"));
        assert!(!valid_repo("acme/a/b"));
        assert!(!valid_repo("/api"));
    }
}
