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
    /// Environment its agents run in. Falls back to the repositories' default.
    #[serde(default)]
    pub environment: Option<String>,
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
    /// Named agents a conductor starts by name, each with its own brief,
    /// skills and permissions.
    #[serde(default)]
    pub agents: BTreeMap<String, AgentSpec>,
    /// An implement-and-review loop over two of those agents, which the
    /// platform counts and bounds.
    #[serde(default, rename = "loop")]
    pub review_loop: Option<LoopSpec>,
    /// Prompt template. `{{name}}` is replaced by the input of that name.
    pub prompt: String,
}

/// A worker a conductor can start by name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSpec {
    /// One line telling the conductor what this agent is for.
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub harness: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// Repositories it checks out. Defaults to those of the agent whose
    /// branch it shares, or else all of the workflow's.
    #[serde(default)]
    pub repos: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    /// The most this agent may do; absent means whatever its conductor may.
    #[serde(default)]
    pub grants: Option<GrantSet>,
    /// Work on the branch of the latest agent started under this name,
    /// instead of a branch of its own.
    #[serde(default)]
    pub branch_of: Option<String>,
    /// Start a new agent each time instead of resuming the last one, so it
    /// is not anchored on what it concluded before.
    #[serde(default)]
    pub fresh: bool,
    /// It must report a verdict (approve, or request changes) before its
    /// work counts as finished.
    #[serde(default)]
    pub verdict: bool,
    /// Its standing brief. `{{name}}` is replaced by the workflow input of
    /// that name; the conductor adds the specifics of each piece of work.
    pub prompt: String,
}

/// Implement, review, and repeat until the reviewer approves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoopSpec {
    /// The agent that does the work.
    pub implement: String,
    /// The agent that reviews it. It reports a verdict, and each verdict
    /// ends a round.
    pub review: String,
    /// Rounds allowed before the platform stops the loop.
    #[serde(default = "default_rounds")]
    pub max_rounds: u32,
}

fn default_rounds() -> u32 {
    5
}

pub const MAX_LOOP_ROUNDS: u32 = 50;

/// What a conductor started from a workflow carries with it: the workflow's
/// agents with their briefs filled in, and its loop.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    #[serde(default)]
    pub agents: BTreeMap<String, AgentSpec>,
    #[serde(default, rename = "loop")]
    pub review_loop: Option<LoopSpec>,
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
    #[error("agent name `{0}` must be lowercase letters, digits and dashes")]
    BadAgentName(String),
    #[error("`agents` and `loop` need `role: conductor`: only a conductor starts other agents")]
    NeedsConductor,
    #[error("agent `{0}` lists repository `{1}`, which is not among the workflow's `repos`")]
    AgentRepo(String, String),
    #[error("{0} names agent `{1}`, but no agent of that name is declared under `agents`")]
    UnknownAgent(String, String),
    #[error("agent `{0}` cannot share its own branch")]
    OwnBranch(String),
    #[error("a loop needs two different agents to implement and to review")]
    LoopSameAgent,
    #[error("loop max_rounds must be between 1 and {MAX_LOOP_ROUNDS}")]
    BadRounds,
}

pub fn valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

/// A repository path: `owner/name`, or deeper where the host nests groups
/// (`group/subgroup/project` on GitLab).
pub fn valid_repo(s: &str) -> bool {
    let ok = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && !p.ends_with(".git")
            && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    s.len() <= 255 && (2..=21).contains(&s.split('/').count()) && s.split('/').all(ok)
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
        let templates = std::iter::once(&self.prompt).chain(self.agents.values().map(|agent| &agent.prompt));
        for name in templates.flat_map(|template| placeholders(template)) {
            if !seen.contains(name.as_str()) {
                return Err(WorkflowError::UndeclaredInput(name));
            }
        }

        if self.role != SessionRole::Conductor && (!self.agents.is_empty() || self.review_loop.is_some()) {
            return Err(WorkflowError::NeedsConductor);
        }
        let declared = |what: String, name: &str| match self.agents.contains_key(name) {
            true => Ok(()),
            false => Err(WorkflowError::UnknownAgent(what, name.to_string())),
        };
        for (name, agent) in &self.agents {
            if !valid_slug(name) {
                return Err(WorkflowError::BadAgentName(name.clone()));
            }
            for repo in &agent.repos {
                if !self.repos.iter().any(|r| r.eq_ignore_ascii_case(repo)) {
                    return Err(WorkflowError::AgentRepo(name.clone(), repo.clone()));
                }
            }
            if let Some(source) = &agent.branch_of {
                if source == name {
                    return Err(WorkflowError::OwnBranch(name.clone()));
                }
                declared(format!("agent `{name}`'s branch_of"), source)?;
            }
        }
        if let Some(review_loop) = &self.review_loop {
            declared("loop.implement".into(), &review_loop.implement)?;
            declared("loop.review".into(), &review_loop.review)?;
            if review_loop.implement == review_loop.review {
                return Err(WorkflowError::LoopSameAgent);
            }
            if !(1..=MAX_LOOP_ROUNDS).contains(&review_loop.max_rounds) {
                return Err(WorkflowError::BadRounds);
            }
        }
        Ok(())
    }

    /// The agents and loop a conductor started from this workflow works
    /// with, briefs filled in. `None` when the workflow declares neither.
    ///
    /// A loop's reviewer always reports a verdict, and reviews the
    /// implementer's branch unless it names another.
    pub fn plan(&self, values: &BTreeMap<String, String>) -> Result<Option<Plan>, WorkflowError> {
        if self.agents.is_empty() {
            return Ok(None);
        }
        let resolved = self.resolve(values)?;
        let mut agents = self.agents.clone();
        for agent in agents.values_mut() {
            agent.prompt = fill(&agent.prompt, &resolved);
        }
        if let Some(review_loop) = &self.review_loop
            && let Some(reviewer) = agents.get_mut(&review_loop.review)
        {
            reviewer.verdict = true;
            reviewer.branch_of.get_or_insert_with(|| review_loop.implement.clone());
        }
        Ok(Some(Plan { agents, review_loop: self.review_loop.clone() }))
    }

    /// Fill the prompt template.
    pub fn render(&self, values: &BTreeMap<String, String>) -> Result<String, WorkflowError> {
        Ok(fill(&self.prompt, &self.resolve(values)?))
    }

    /// The value of every declared input, given what was supplied.
    fn resolve(&self, values: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>, WorkflowError> {
        for key in values.keys() {
            if !self.inputs.iter().any(|i| &i.name == key) {
                return Err(WorkflowError::UnknownInput(key.clone()));
            }
        }
        let mut resolved = BTreeMap::new();
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
            resolved.insert(input.name.clone(), value);
        }
        Ok(resolved)
    }
}

/// Replace each `{{name}}` with its value. Single pass, so braces inside a
/// substituted value are never expanded.
fn fill(template: &str, resolved: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
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
    out
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
        assert!(wf.grants.has(&Grant::GITHUB_READ));
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

    const LOOP: &str = r#"
name: build-and-review
role: conductor
repos: [acme/api]
grants: [github:write, github:open_pr]
inputs:
  - name: goal
agents:
  implementer:
    prompt: "Implement: {{goal}}"
  reviewer:
    grants: [github:read]
    fresh: true
    prompt: "Review against: {{goal}}"
loop:
  implement: implementer
  review: reviewer
  max_rounds: 3
prompt: "Goal: {{goal}}"
"#;

    #[test]
    fn a_loop_fills_in_its_agents() {
        let wf = WorkflowSpec::from_yaml(LOOP).unwrap();
        let mut values = BTreeMap::new();
        values.insert("goal".to_string(), "dark mode".to_string());
        let plan = wf.plan(&values).unwrap().unwrap();
        assert_eq!(plan.agents["implementer"].prompt, "Implement: dark mode");
        let reviewer = &plan.agents["reviewer"];
        assert_eq!(reviewer.prompt, "Review against: dark mode");
        // The loop's reviewer reports a verdict on the implementer's branch.
        assert!(reviewer.verdict && reviewer.fresh);
        assert_eq!(reviewer.branch_of.as_deref(), Some("implementer"));
        assert!(!reviewer.grants.as_ref().unwrap().has(&Grant::GITHUB_WRITE));
        assert_eq!(plan.review_loop.unwrap().max_rounds, 3);
        // It survives being stored with the session.
        let stored = serde_json::to_value(wf.plan(&values).unwrap().unwrap()).unwrap();
        assert_eq!(serde_json::from_value::<Plan>(stored).unwrap().agents.len(), 2);

        assert_eq!(WorkflowSpec::from_yaml(YAML).unwrap().plan(&values_for("feature")).unwrap(), None);
    }

    fn values_for(name: &str) -> BTreeMap<String, String> {
        BTreeMap::from([(name.to_string(), "x".to_string())])
    }

    #[test]
    fn rejects_loops_that_cannot_run() {
        let err = |yaml: String| WorkflowSpec::from_yaml(&yaml).unwrap_err();
        assert_eq!(err(LOOP.replace("role: conductor", "role: worker")), WorkflowError::NeedsConductor);
        assert_eq!(
            err(LOOP.replace("review: reviewer", "review: critic")),
            WorkflowError::UnknownAgent("loop.review".into(), "critic".into())
        );
        assert_eq!(err(LOOP.replace("review: reviewer", "review: implementer")), WorkflowError::LoopSameAgent);
        assert_eq!(err(LOOP.replace("max_rounds: 3", "max_rounds: 0")), WorkflowError::BadRounds);
        assert_eq!(err(LOOP.replace("Review against: {{goal}}", "Review {{nope}}")), WorkflowError::UndeclaredInput("nope".into()));
        assert_eq!(
            err(LOOP.replace("fresh: true", "repos: [acme/web]")),
            WorkflowError::AgentRepo("reviewer".into(), "acme/web".into())
        );
        assert_eq!(err(LOOP.replace("fresh: true", "branch_of: reviewer")), WorkflowError::OwnBranch("reviewer".into()));
        assert_eq!(err(LOOP.replace("  reviewer:", "  Reviewer:").replace("review: reviewer", "review: Reviewer")), WorkflowError::BadAgentName("Reviewer".into()));
    }

    #[test]
    fn repo_names() {
        assert!(valid_repo("acme/api.v2"));
        assert!(!valid_repo("acme/../etc"));
        assert!(valid_repo("acme/platform/api"));
        assert!(!valid_repo("acme"));
        assert!(!valid_repo("acme//api"));
        assert!(!valid_repo("acme/api.git"));
        assert!(!valid_repo("/api"));
    }
}
