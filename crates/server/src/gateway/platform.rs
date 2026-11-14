//! Tools for the platform itself: shared team notes, workstream updates,
//! questions to the people an agent works for, and (for conductors) managing
//! worker agents.

use super::{ToolDef, ToolError, ToolOutput, ToolResult, arg_list, arg_opt, arg_str, schema};
use crate::app::App;
use crate::error::ApiError;
use crate::sessions::NewSession;
use crate::store::{self, Link, Session};
use hyperbeam_core::protocol::SessionStatus;
use hyperbeam_core::workflow::SessionRole;
use serde_json::{Value, json};
use uuid::Uuid;

const MAX_MEMORY_BYTES: usize = 64 * 1024;
const MAX_QUESTION_CHARS: usize = 2000;

pub fn tools() -> Vec<ToolDef> {
    let tool = |name, description, schema, conductor_only| ToolDef { name, description, schema, grant: None, conductor_only };
    vec![
        tool("hyperbeam_memory_list", "List the team's shared notes: durable knowledge kept across sessions.", schema(&[]), false),
        tool("hyperbeam_memory_read", "Read one shared note.", schema(&[("key", "string", "Note key", true)]), false),
        tool(
            "hyperbeam_memory_write",
            "Create or replace a shared note. Use for durable facts future sessions will need (conventions, decisions, how to run things), not for progress updates.",
            schema(&[("key", "string", "Short kebab-case key, e.g. api-testing", true), ("content", "string", "The note (markdown)", true)]),
            false,
        ),
        tool(
            "hyperbeam_update_workstream",
            "Update the workstream this session belongs to: move it between stages or attach a link the team should see.",
            schema(&[
                ("status", "string", "scoping, building, review or done", false),
                ("link_title", "string", "Title of a link to attach", false),
                ("link_url", "string", "URL of the link to attach", false),
            ]),
            false,
        ),
        tool(
            "hyperbeam_ask_user",
            "Put a question to whoever directs this session: the people you work for, or your conductor if one started you. \
             Use it for decisions that are theirs to make, not for things you can find out yourself. It does not wait: \
             the answer arrives later as your next message, so push your work and end your turn after asking.",
            schema(&[
                ("question", "string", "One clear question, with the context needed to answer it", true),
                ("options", "string[]", "Up to six short answers to choose from. People can always answer in their own words.", false),
            ]),
            false,
        ),
        tool(
            "hyperbeam_spawn_agent",
            "Start a worker agent in its own sandbox. It does not see this conversation: the prompt must be a complete brief.",
            schema(&[
                ("title", "string", "Short name for the piece of work", true),
                ("prompt", "string", "Complete instructions: goal, context, constraints, definition of done", true),
                ("repos", "string[]", "Repositories (owner/name) to check out; must be among this session's", false),
                ("skills", "string[]", "Skill names to install", false),
                ("harness", "string", "Harness profile name; defaults to this session's", false),
                ("model", "string", "Model for the worker; defaults to this session's. Use a cheaper model for routine pieces.", false),
            ]),
            true,
        ),
        tool("hyperbeam_list_agents", "List the worker agents this conductor has started, with status.", schema(&[]), true),
        tool("hyperbeam_get_agent", "Status, latest report and links for one worker agent.", schema(&[("id", "string", "Agent session id", true)]), true),
        tool(
            "hyperbeam_message_agent",
            "Send a follow-up prompt to a worker agent. A finished agent is resumed.",
            schema(&[("id", "string", "Agent session id", true), ("text", "string", "The message", true)]),
            true,
        ),
        tool("hyperbeam_stop_agent", "Stop a worker agent and tear down its sandbox.", schema(&[("id", "string", "Agent session id", true)]), true),
    ]
}

impl From<ApiError> for ToolError {
    fn from(err: ApiError) -> Self {
        match err {
            ApiError::Forbidden(why) => ToolError::Denied(why),
            ApiError::Internal(err) => {
                tracing::error!("gateway internal error: {err:#}");
                ToolError::Failed("the platform hit an internal error".into())
            }
            other => ToolError::Failed(other.to_string()),
        }
    }
}

async fn child(app: &App, session: &Session, args: &Value) -> Result<Session, ToolError> {
    let id: Uuid = arg_str(args, "id")?.parse().map_err(|_| ToolError::Failed("`id` is not a session id".into()))?;
    match store::session_by_id(&app.db, id).await? {
        Some(found) if found.parent_id == Some(session.id) => Ok(found),
        _ => Err(ToolError::Denied("that is not one of this conductor's agents".into())),
    }
}

fn describe(agent: &Session) -> String {
    let mut text = format!("{} — “{}” [{}]", agent.id, agent.title, agent.status);
    if !agent.headline.is_empty() && matches!(agent.status.as_str(), "running" | "starting") {
        text.push_str(&format!("\n  now: {}", agent.headline));
    }
    if let Some(error) = &agent.error {
        text.push_str(&format!("\n  error: {error}"));
    }
    for link in agent.links.iter() {
        text.push_str(&format!("\n  {}: {}", link.kind, link.url));
    }
    text
}

pub async fn call(app: &App, session: &Session, name: &str, args: &Value) -> ToolResult {
    match name {
        "hyperbeam_memory_list" => {
            let rows: Vec<(String, i32)> = sqlx::query_as("SELECT key, length(content) FROM memory ORDER BY key").fetch_all(&app.db).await?;
            let text = if rows.is_empty() {
                "No shared notes yet.".to_string()
            } else {
                rows.iter().map(|(key, len)| format!("{key} ({len} chars)")).collect::<Vec<_>>().join("\n")
            };
            Ok(ToolOutput::new("", text))
        }
        "hyperbeam_memory_read" => {
            let key = arg_str(args, "key")?;
            let row: Option<(String,)> = sqlx::query_as("SELECT content FROM memory WHERE key = $1").bind(key).fetch_optional(&app.db).await?;
            match row {
                Some((content,)) => Ok(ToolOutput::new(key, content)),
                None => Err(ToolError::Failed(format!("no note named `{key}`"))),
            }
        }
        "hyperbeam_memory_write" => {
            let key = arg_str(args, "key")?;
            let content = arg_str(args, "content")?;
            if !hyperbeam_core::workflow::valid_slug(key) {
                return Err(ToolError::Failed("key must be lowercase letters, digits and dashes (max 64)".into()));
            }
            if content.len() > MAX_MEMORY_BYTES {
                return Err(ToolError::Failed(format!("note is too large (limit {MAX_MEMORY_BYTES} bytes); split it")));
            }
            sqlx::query(
                "INSERT INTO memory (key, content, updated_by_session) VALUES ($1, $2, $3)
                 ON CONFLICT (key) DO UPDATE SET content = excluded.content, updated_by_session = excluded.updated_by_session,
                                                 updated_by_user = NULL, updated_at = now()",
            )
            .bind(key)
            .bind(content)
            .bind(session.id)
            .execute(&app.db)
            .await?;
            app.activity(None, Some(session), None, "memory.updated", &format!("“{}” updated team note `{key}`", session.title), json!({ "key": key })).await;
            Ok(ToolOutput::new(key, format!("Saved note `{key}`.")))
        }
        "hyperbeam_update_workstream" => {
            let Some(ws) = session.workstream_id else {
                return Err(ToolError::Failed("this session does not belong to a workstream".into()));
            };
            let mut done = Vec::new();
            if let Some(status) = arg_opt(args, "status") {
                if !["scoping", "building", "review", "done"].contains(&status) {
                    return Err(ToolError::Failed("status must be scoping, building, review or done".into()));
                }
                sqlx::query("UPDATE workstreams SET status = $2, updated_at = now() WHERE id = $1").bind(ws).bind(status).execute(&app.db).await?;
                app.activity(None, Some(session), None, "workstream.status", &format!("“{}” moved its workstream to {status}", session.title), json!({ "status": status })).await;
                done.push(format!("status → {status}"));
            }
            if let (Some(title), Some(url)) = (arg_opt(args, "link_title"), arg_opt(args, "link_url")) {
                if !(url.starts_with("https://") || url.starts_with("http://")) {
                    return Err(ToolError::Failed("link_url must be an http(s) URL".into()));
                }
                let link = json!([Link { kind: "link".into(), title: title.into(), url: url.into() }]);
                sqlx::query("UPDATE workstreams SET links = links || $2, updated_at = now() WHERE id = $1 AND NOT links @> $2")
                    .bind(ws)
                    .bind(link)
                    .execute(&app.db)
                    .await?;
                done.push(format!("linked {url}"));
            }
            if done.is_empty() {
                return Err(ToolError::Failed("pass status, or link_title and link_url".into()));
            }
            app.publish("workstream", None, json!({ "id": ws }));
            Ok(ToolOutput::new("workstream", format!("Workstream updated: {}.", done.join(", "))))
        }
        "hyperbeam_ask_user" => {
            let text = arg_str(args, "question")?;
            if text.chars().count() > MAX_QUESTION_CHARS {
                return Err(ToolError::Failed(format!("question is too long (limit {MAX_QUESTION_CHARS} characters); put the detail in your message and ask something short")));
            }
            let options: Vec<String> = arg_list(args, "options").iter().map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
            if options.len() > 6 || options.iter().any(|o| o.chars().count() > 200) {
                return Err(ToolError::Failed("give at most six options of at most 200 characters each".into()));
            }
            let question = app.ask_question(session, text, options).await?;
            let who = if question.to == "conductor" { "your conductor" } else { "the people you work for" };
            Ok(ToolOutput::new(
                text.chars().take(120).collect::<String>(),
                format!(
                    "Question sent to {who}. This call does not wait for the answer: finish anything that does not depend on it, \
                     push work worth keeping, then end your turn. The answer will arrive as your next message. Do not ask it again."
                ),
            ))
        }
        "hyperbeam_spawn_agent" => {
            let title = arg_str(args, "title")?;
            let prompt = arg_str(args, "prompt")?;
            let repos = arg_list(args, "repos");
            for repo in &repos {
                if !session.repos.iter().any(|r| r.eq_ignore_ascii_case(repo)) {
                    return Err(ToolError::Denied(format!(
                        "`{repo}` is not one of this session's repositories ({}); a worker cannot reach further than its conductor",
                        session.repos.join(", ")
                    )));
                }
            }
            let user = store::user_by_id(&app.db, session.created_by)
                .await?
                .filter(|u| !u.disabled)
                .ok_or_else(|| ToolError::Denied("the person who started this session no longer has access".into()))?;
            let harness = match arg_opt(args, "harness") {
                Some(name) => name.to_string(),
                None => store::harness_by_id(&app.db, session.harness_id).await?.map(|h| h.name).unwrap_or_default(),
            };
            let worker = app
                .create_session(
                    &user,
                    NewSession {
                        title: Some(title.to_string()),
                        prompt: prompt.to_string(),
                        workstream_id: session.workstream_id,
                        parent: Some(session.clone()),
                        role: SessionRole::Worker,
                        harness: Some(harness),
                        repos,
                        skills: arg_list(args, "skills"),
                        grants: None,
                        workflow: None,
                        // Same harness: inherit the conductor's choice unless told otherwise.
                        model: arg_opt(args, "model").map(str::to_string).or_else(|| {
                            arg_opt(args, "harness").is_none().then(|| session.model.clone()).flatten()
                        }),
                        // Settled from the worker's repositories and this session's own.
                        environment: None,
                        context: None,
                    },
                )
                .await?;
            Ok(ToolOutput::new(
                title,
                format!(
                    "Started worker agent {} (“{}”) on branch {}. You will be prompted when it finishes a turn; end your turn once you have delegated everything you can.",
                    worker.id, worker.title, worker.work_branch
                ),
            ))
        }
        "hyperbeam_list_agents" => {
            let agents: Vec<Session> = sqlx::query_as(&format!("SELECT {} FROM sessions WHERE parent_id = $1 ORDER BY created_at", store::SESSION_COLUMNS))
                .bind(session.id)
                .fetch_all(&app.db)
                .await?;
            let text = if agents.is_empty() { "No worker agents yet.".to_string() } else { agents.iter().map(describe).collect::<Vec<_>>().join("\n") };
            Ok(ToolOutput::new("", text))
        }
        "hyperbeam_get_agent" => {
            let agent = child(app, session, args).await?;
            let mut text = describe(&agent);
            text.push_str(&format!("\n  branch: {}", agent.work_branch));
            if !agent.summary.is_empty() {
                text.push_str(&format!("\n\nLatest report:\n{}", agent.summary));
            }
            Ok(ToolOutput::new(agent.title, text))
        }
        "hyperbeam_message_agent" => {
            let agent = child(app, session, args).await?;
            let text = arg_str(args, "text")?;
            // Whatever the conductor says next is its answer to a waiting worker.
            app.clear_question(agent.id, None).await?;
            app.send_input(&agent, None, &format!("{} (conductor)", session.title), text).await?;
            Ok(ToolOutput::new(agent.title, "Message delivered. You will be prompted when the agent finishes its turn.".to_string()))
        }
        "hyperbeam_stop_agent" => {
            let agent = child(app, session, args).await?;
            app.end_session(agent.id, SessionStatus::Cancelled, None).await;
            Ok(ToolOutput::new(agent.title.clone(), format!("Stopped “{}”.", agent.title)))
        }
        other => Err(ToolError::Failed(format!("no tool named `{other}`"))),
    }
}
