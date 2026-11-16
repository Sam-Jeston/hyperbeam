//! Tools for the platform itself: shared team notes, the knowledge base,
//! workstream updates, questions to the people an agent works for, and (for conductors) managing
//! worker agents.

use super::{ToolDef, ToolError, ToolOutput, ToolResult, arg_list, arg_opt, arg_str, schema};
use crate::app::App;
use crate::error::ApiError;
use crate::knowledge::{self, Config};
use crate::review;
use crate::sessions::NewSession;
use crate::store::{self, Link, Session};
use hyperbeam_core::protocol::SessionStatus;
use hyperbeam_core::GrantSet;
use hyperbeam_core::workflow::SessionRole;
use serde_json::{Value, json};
use uuid::Uuid;

pub const REPORT_VERDICT: &str = "hyperbeam_report_verdict";
const MAX_FINDINGS_CHARS: usize = 20_000;
const MAX_MEMORY_BYTES: usize = 64 * 1024;
const MAX_QUESTION_CHARS: usize = 2000;
/// How much of a document one read returns.
const MAX_READ_CHARS: usize = 80_000;

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
            "hyperbeam_knowledge_publish",
            "Publish a document to the team's knowledge base: a report, an investigation's findings, a design, a write-up. \
             Use it for anything written for people to read, instead of leaving a file in the sandbox, where it is lost. \
             Returns the document's link; give that link in your reply.",
            schema(&[
                ("title", "string", "What the document is, as a reader would look for it, e.g. Checkout latency investigation", true),
                ("content", "string", "The whole document (markdown)", true),
                ("summary", "string", "One sentence saying what it found or decided, shown in lists and search results", false),
                ("id", "string", "To replace a document this work published earlier, its id", false),
            ]),
            false,
        ),
        tool(
            "hyperbeam_knowledge_search",
            "Search the knowledge base for documents earlier sessions and people published. With no query, lists the newest.",
            schema(&[("query", "string", "Words to look for", false)]),
            false,
        ),
        tool("hyperbeam_knowledge_read", "Read one document from the knowledge base.", schema(&[("id", "string", "Document id, from a search", true)]), false),
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
            "Start a worker agent in its own sandbox. It does not see this conversation: the prompt must be a complete brief. \
             If your workflow declares agents, pass one's name as `agent` to start it with its standing brief, skills and permissions.",
            schema(&[
                ("title", "string", "Short name for the piece of work. Required unless `agent` is given.", false),
                (
                    "prompt",
                    "string",
                    "Complete instructions: goal, context, constraints, definition of done. With `agent`, only what is particular to this piece of work, added to its standing brief.",
                    false,
                ),
                ("agent", "string", "Name of one of your workflow's declared agents", false),
                ("repos", "string[]", "Repositories (owner/name) to check out; must be among this session's", false),
                ("skills", "string[]", "Skill names to install", false),
                (
                    "grants",
                    "string[]",
                    "The most the worker may do, e.g. [\"github:read\"] for one that must not change anything. Defaults to what you may do; it can only narrow that.",
                    false,
                ),
                (
                    "branch_of",
                    "string",
                    "Id of another of your workers: this one checks out that worker's branch instead of getting its own, to review or continue its work",
                    false,
                ),
                (
                    "verdict",
                    "boolean",
                    "The worker must end by reporting a verdict, approve or request_changes, which arrives with its report. Use for reviews.",
                    false,
                ),
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
        tool(
            REPORT_VERDICT,
            "Report the verdict of your review. Call it once you have finished looking, then end your turn; your conductor acts on it.",
            schema(&[
                ("verdict", "string", "`approve` if the work can ship as it stands, otherwise `request_changes`", true),
                (
                    "findings",
                    "string",
                    "For request_changes: what must change and where, specifically enough for another agent to act on. For approve: what you checked.",
                    true,
                ),
            ]),
            false,
        ),
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
        "hyperbeam_knowledge_publish" => {
            let title = arg_str(args, "title")?;
            let revise = match arg_opt(args, "id") {
                Some(id) => Some(id.parse::<Uuid>().map_err(|_| ToolError::Failed("`id` is not a document id".into()))?),
                None => None,
            };
            let published = app.publish_document(session, title, arg_opt(args, "summary"), arg_str(args, "content")?, revise).await?;
            if !published.revised {
                let link = Link { kind: "document".into(), title: published.title.clone(), url: published.url.clone() };
                super::attach_link(app, session, &link).await;
            }
            let verb = if published.revised { "revised" } else { "published" };
            app.activity(
                None,
                Some(session),
                None,
                "document.published",
                &format!("“{}” {verb} “{}”", session.title, published.title),
                json!({ "document_id": published.id, "url": published.url, "title": published.title }),
            )
            .await;
            let text = match published.id {
                Some(id) => format!(
                    "{} “{}” in the knowledge base: {}\nIts id is {id}; pass that as `id` to replace it later. Give the link in your reply.",
                    if published.revised { "Revised" } else { "Published" },
                    published.title,
                    published.url
                ),
                None => format!("Published “{}” to the team's knowledge base in Notion: {}\nGive the link in your reply.", published.title, published.url),
            };
            Ok(ToolOutput::new(published.title, text))
        }
        "hyperbeam_knowledge_search" | "hyperbeam_knowledge_read" if Config::load(&app.db).await?.notion_target().is_some() => Err(ToolError::Failed(
            "the team's knowledge base is in Notion: search and read it with the `notion_*` tools, if this session has them".into(),
        )),
        "hyperbeam_knowledge_search" => {
            let query = arg_opt(args, "query").unwrap_or_default();
            let found = knowledge::find(&app.db, query, None, 20).await?;
            let text = if found.is_empty() {
                if query.is_empty() { "The knowledge base is empty.".to_string() } else { format!("No documents match “{query}”.") }
            } else {
                found
                    .iter()
                    .map(|d| format!("{} — {} ({})\n  {}", d.id, d.title, d.updated_at.format("%Y-%m-%d"), d.summary))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            Ok(ToolOutput::new(query, text))
        }
        "hyperbeam_knowledge_read" => {
            let id: Uuid = arg_str(args, "id")?.parse().map_err(|_| ToolError::Failed("`id` is not a document id".into()))?;
            match knowledge::by_id(&app.db, id).await? {
                Some(doc) => Ok(ToolOutput::new(doc.title.clone(), format!("# {}\n\n{}", doc.title, super::clip(&doc.content, MAX_READ_CHARS)))),
                None => Err(ToolError::Failed(format!("no document with id `{id}`"))),
            }
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
                if matches!(status, "review" | "done")
                    && let Some(state) = review::loop_around(&app.db, session).await?
                    && state.standing != review::Standing::Approved
                {
                    return Err(ToolError::Denied(format!(
                        "this work runs a loop, and `{}` has not approved it (round {} of {}); the workstream stays where it is until it does",
                        state.review, state.rounds, state.max_rounds
                    )));
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
            // A declared agent brings its own brief, skills and limits; the
            // arguments add to them, and can narrow but never widen.
            let plan = review::plan_of(&app.db, session.id).await?.unwrap_or_default();
            let name = arg_opt(args, "agent");
            let spec = match name {
                Some(name) => plan.agents.get(name).cloned().ok_or_else(|| {
                    let declared: Vec<String> = plan.agents.keys().map(|name| format!("`{name}`")).collect();
                    ToolError::Failed(match declared.is_empty() {
                        true => format!("this session's workflow declares no agents, so there is no `{name}`; leave `agent` out and give a title and a full prompt"),
                        false => format!("no agent named `{name}`; this workflow declares {}", declared.join(", ")),
                    })
                })?,
                None => Default::default(),
            };
            let state = review::loop_state(&app.db, session.id).await?.filter(|state| state.covers(name));
            if let Some(state) = &state
                && state.exhausted()
            {
                return Err(ToolError::Denied(state.refusal()));
            }

            let brief = [spec.prompt.trim(), arg_opt(args, "prompt").unwrap_or_default()].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("\n\n");
            if brief.is_empty() {
                return Err(ToolError::Failed("missing required argument `prompt`".into()));
            }
            let title = match (arg_opt(args, "title"), name, &state) {
                (Some(title), _, _) => title.to_string(),
                (None, Some(name), Some(state)) if name == state.review => format!("{name} (round {})", state.rounds + 1),
                (None, Some(name), _) => name.to_string(),
                (None, None, _) => return Err(ToolError::Failed("missing required argument `title`".into())),
            };

            let shares = match (arg_opt(args, "branch_of"), &spec.branch_of) {
                (Some(_), _) => Some(child(app, session, &json!({ "id": args["branch_of"] })).await?),
                (None, Some(other)) => {
                    let latest: Option<Session> = sqlx::query_as(&format!(
                        "SELECT {} FROM sessions WHERE parent_id = $1 AND agent = $2 ORDER BY created_at DESC LIMIT 1",
                        store::SESSION_COLUMNS
                    ))
                    .bind(session.id)
                    .bind(other)
                    .fetch_optional(&app.db)
                    .await?;
                    Some(latest.ok_or_else(|| {
                        ToolError::Failed(format!(
                            "`{}` works on the branch of `{other}`, and you have not started a `{other}` yet. Start it first, and wait for it to report that its work is pushed.",
                            name.unwrap_or_default()
                        ))
                    })?)
                }
                (None, None) => None,
            };

            let repos = match (arg_list(args, "repos"), &shares) {
                (asked, _) if !asked.is_empty() => asked,
                _ if !spec.repos.is_empty() => spec.repos.clone(),
                (_, Some(other)) => other.repos.0.clone(),
                // A declared agent works on the workflow's repositories.
                _ if name.is_some() => session.repos.0.clone(),
                _ => Vec::new(),
            };
            for repo in &repos {
                if !session.repos.iter().any(|r| r.eq_ignore_ascii_case(repo)) {
                    return Err(ToolError::Denied(format!(
                        "`{repo}` is not one of this session's repositories ({}); a worker cannot reach further than its conductor",
                        session.repos.join(", ")
                    )));
                }
                if let Some(other) = &shares
                    && !other.repos.iter().any(|r| r.eq_ignore_ascii_case(repo))
                {
                    return Err(ToolError::Failed(format!(
                        "“{}” does not work on `{repo}`, so its branch `{}` is not there to share",
                        other.title, other.work_branch
                    )));
                }
            }

            let asked = arg_list(args, "grants");
            let asked = match asked.is_empty() {
                true => None,
                false => Some(GrantSet::parse_many(&asked).map_err(|err| ToolError::Failed(format!("in `grants`: {err}")))?),
            };
            let grants = match (spec.grants.clone(), asked) {
                (Some(declared), Some(asked)) => Some(declared.intersect(&asked)),
                (declared, asked) => declared.or(asked),
            };
            let mut skills = spec.skills.clone();
            skills.extend(arg_list(args, "skills"));
            let reports_verdict = spec.verdict || args.get("verdict").and_then(Value::as_bool).unwrap_or(false);

            let user = store::user_by_id(&app.db, session.created_by)
                .await?
                .filter(|u| !u.disabled)
                .ok_or_else(|| ToolError::Denied("the person who started this session no longer has access".into()))?;
            let chosen = arg_opt(args, "harness").map(str::to_string).or_else(|| spec.harness.clone());
            let harness = match &chosen {
                Some(name) => name.clone(),
                None => store::harness_by_id(&app.db, session.harness_id).await?.map(|h| h.name).unwrap_or_default(),
            };
            let worker = app
                .create_session(
                    &user,
                    NewSession {
                        title: Some(title.clone()),
                        prompt: brief,
                        workstream_id: session.workstream_id,
                        parent: Some(session.clone()),
                        role: SessionRole::Worker,
                        harness: Some(harness),
                        repos,
                        skills,
                        grants,
                        workflow: None,
                        plan: None,
                        agent: name.map(str::to_string),
                        reports_verdict,
                        branch_of: shares.clone(),
                        // Same harness: inherit the conductor's choice unless told otherwise.
                        model: arg_opt(args, "model")
                            .map(str::to_string)
                            .or_else(|| spec.model.clone())
                            .or_else(|| chosen.is_none().then(|| session.model.clone()).flatten()),
                        // Settled from the worker's repositories and this session's own.
                        environment: None,
                        context: None,
                    },
                )
                .await?;
            let mut text = match &shares {
                Some(other) => format!(
                    "Started worker agent {} (“{}”) on branch {}, which it shares with “{}”.",
                    worker.id, worker.title, worker.work_branch, other.title
                ),
                None => format!("Started worker agent {} (“{}”) on branch {}.", worker.id, worker.title, worker.work_branch),
            };
            if reports_verdict {
                text.push_str(" It will report a verdict, which arrives with its report.");
            }
            text.push_str(" You will be prompted when it finishes a turn; end your turn once you have delegated everything you can.");
            Ok(ToolOutput::new(title, text))
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
            if let Some(state) = review::loop_state(&app.db, session.id).await?
                && state.covers(agent.agent.as_deref())
                && state.exhausted()
            {
                return Err(ToolError::Denied(state.refusal()));
            }
            // An agent declared `fresh` gives one verdict and is then done.
            if let Some(name) = &agent.agent
                && review::plan_of(&app.db, session.id).await?.is_some_and(|plan| plan.agents.get(name).is_some_and(|spec| spec.fresh))
                && !review::latest_verdicts(&app.db, &[agent.id]).await?.is_empty()
            {
                return Err(ToolError::Failed(format!(
                    "“{}” has given its verdict, and `{name}` is started anew each time rather than resumed: start another with `hyperbeam_spawn_agent`",
                    agent.title
                )));
            }
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
        REPORT_VERDICT => {
            let verdict = match arg_str(args, "verdict")? {
                "approve" | "approved" => review::APPROVE,
                "request_changes" | "changes_requested" => review::REQUEST_CHANGES,
                _ => return Err(ToolError::Failed("verdict must be `approve` or `request_changes`".into())),
            };
            let findings = match (arg_opt(args, "findings"), verdict) {
                (Some(findings), _) => super::clip(findings, MAX_FINDINGS_CHARS),
                (None, review::APPROVE) => "Approved.".to_string(),
                (None, _) => return Err(ToolError::Failed("say what must change in `findings`".into())),
            };
            let recorded = app.record_verdict(session, verdict, &findings).await.map_err(ApiError::from)?;
            let to = if session.parent_id.is_some() { "Your conductor is told when you end your turn" } else { "It is shown on this session" };
            Ok(ToolOutput::new(recorded.past_tense(), format!("Verdict recorded: {}. {to}. End your turn now.", recorded.verdict)))
        }
        other => Err(ToolError::Failed(format!("no tool named `{other}`"))),
    }
}
