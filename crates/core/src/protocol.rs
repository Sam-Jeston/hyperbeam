//! Wire protocol between the control plane and the in-sandbox runner.
//!
//! The runner dials out to the server over a WebSocket and exchanges JSON
//! text frames. The server never connects into a sandbox.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;

/// Everything a runner needs to prepare a workspace and start a harness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSpec {
    pub session_id: Uuid,
    pub title: String,
    pub harness: HarnessSpec,
    #[serde(default)]
    pub repos: Vec<RepoSpec>,
    #[serde(default)]
    pub skills: Vec<SkillSpec>,
    /// Tool gateway (MCP over streamable HTTP). Authenticated with the run token.
    pub mcp_url: String,
    /// Base URL of the git smart-HTTP proxy, e.g. `http://server/git`.
    pub git_base_url: String,
    /// Context the platform prepends to the first prompt of a harness process.
    pub preamble: String,
    pub git_user_name: String,
    pub git_user_email: String,
    /// Branch the agent should do its work on.
    pub work_branch: String,
    /// Start the harness only to learn what it offers (its models): report
    /// `Ready` and exit, without a workspace, tools or prompts.
    #[serde(default)]
    pub probe: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessSpec {
    pub name: String,
    /// argv that starts an ACP agent speaking JSON-RPC on stdio.
    pub command: Vec<String>,
    /// Extra environment, including the harness credential.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Model to select through the harness's ACP `model` option. `None`
    /// leaves the harness on its own default.
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoSpec {
    /// `owner/name`
    pub full_name: String,
    pub default_branch: String,
}

impl RepoSpec {
    /// Directory name inside the workspace.
    pub fn dir_name(&self) -> &str {
        self.full_name.rsplit('/').next().unwrap_or(&self.full_name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSpec {
    pub name: String,
    pub description: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Spec { spec: Box<RunSpec> },
    Prompt { input_id: Uuid, text: String },
    /// Cancel the turn in progress.
    Cancel,
    /// Stop the harness and exit.
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunnerMsg {
    Hello {
        protocol: u32,
        runner_version: String,
        /// True when reconnecting after a dropped connection: the runner
        /// already holds its spec and must not be sent another.
        #[serde(default)]
        resumed: bool,
    },
    /// Workspace prepared and harness session established.
    Ready { agent: Value },
    TurnStarted { input_id: Uuid },
    TurnEnded { input_id: Uuid, stop_reason: String },
    Event { kind: String, data: Value },
    /// Unrecoverable failure; the runner exits after sending this.
    Failed { error: String },
}

/// Event kinds stored in a session's log. The log is the product: it is what
/// teammates watch.
pub mod event {
    pub const USER_MESSAGE: &str = "user_message";
    pub const AGENT_MESSAGE: &str = "agent_message";
    pub const AGENT_THOUGHT: &str = "agent_thought";
    pub const TOOL_CALL: &str = "tool_call";
    pub const TOOL_CALL_UPDATE: &str = "tool_call_update";
    pub const PLAN: &str = "plan";
    pub const PERMISSION: &str = "permission";
    /// The agent asked the people it works for (or its conductor) a question.
    pub const QUESTION: &str = "question";
    /// Session lifecycle transition.
    pub const STATUS: &str = "status";
    /// Platform notice (workspace setup, relaunch, teardown).
    pub const SYSTEM: &str = "system";
    /// A call through the tool gateway or git proxy, allowed or denied.
    pub const GATEWAY: &str = "gateway";
    pub const ERROR: &str = "error";
    pub const TURN_END: &str = "turn_end";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// Waiting for a sandbox.
    Pending,
    /// Sandbox created; runner preparing the workspace.
    Starting,
    /// A turn is in progress.
    Running,
    /// Alive and waiting for input.
    Idle,
    /// Sandbox torn down; can be resumed with a new prompt.
    Completed,
    Failed,
    Cancelled,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionStatus::Pending => "pending",
            SessionStatus::Starting => "starting",
            SessionStatus::Running => "running",
            SessionStatus::Idle => "idle",
            SessionStatus::Completed => "completed",
            SessionStatus::Failed => "failed",
            SessionStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => SessionStatus::Pending,
            "starting" => SessionStatus::Starting,
            "running" => SessionStatus::Running,
            "idle" => SessionStatus::Idle,
            "completed" => SessionStatus::Completed,
            "failed" => SessionStatus::Failed,
            "cancelled" => SessionStatus::Cancelled,
            _ => return None,
        })
    }

    /// Does the session currently own a sandbox?
    pub fn is_live(self) -> bool {
        matches!(
            self,
            SessionStatus::Pending | SessionStatus::Starting | SessionStatus::Running | SessionStatus::Idle
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_tagged() {
        let msg = ServerMsg::Prompt { input_id: Uuid::nil(), text: "hi".into() };
        let v: Value = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["type"], "prompt");
        let back: ServerMsg = serde_json::from_value(v).unwrap();
        assert!(matches!(back, ServerMsg::Prompt { .. }));
    }

    #[test]
    fn repo_dir_name() {
        let r = RepoSpec { full_name: "acme/api".into(), default_branch: "main".into() };
        assert_eq!(r.dir_name(), "api");
    }
}
