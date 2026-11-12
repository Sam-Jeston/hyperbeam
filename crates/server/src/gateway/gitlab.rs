use super::{ToolDef, ToolError, ToolOutput, ToolResult, arg_list, arg_opt, arg_str, arg_u64, attach_link, clip, s, schema, session_repo};
use crate::app::App;
use crate::integrations::Host;
use crate::store::{self, Link, Session};
use hyperbeam_core::grants::GITLAB;
use hyperbeam_core::{Grant, Level};
use reqwest::Method;
use serde_json::{Value, json};

const REPO: (&str, &str, &str, bool) =
    ("repo", "string", "Project path, as group/project. Optional when the session has exactly one GitLab project.", false);

pub fn tools() -> Vec<ToolDef> {
    let tool = |name, description, schema, level| ToolDef { name, description, schema, grant: Some(Grant::on_host(GITLAB, level)), conductor_only: false };
    vec![
        tool(
            "gitlab_list_issues",
            "List issues in a project.",
            schema(&[REPO, ("state", "string", "opened (default), closed or all", false), ("labels", "string", "Comma-separated label names", false)]),
            Level::Read,
        ),
        tool(
            "gitlab_get_issue",
            "Read an issue: title, description and comments.",
            schema(&[REPO, ("number", "integer", "Issue number (its iid, as in #12)", true)]),
            Level::Read,
        ),
        tool(
            "gitlab_list_merge_requests",
            "List merge requests in a project.",
            schema(&[REPO, ("state", "string", "opened (default), closed, merged or all", false)]),
            Level::Read,
        ),
        tool(
            "gitlab_get_merge_request",
            "Read a merge request: description, changed files, approvals, comments and pipeline status.",
            schema(&[REPO, ("number", "integer", "Merge request number (its iid, as in !12)", true)]),
            Level::Read,
        ),
        tool(
            "gitlab_create_issue",
            "Open a new issue.",
            schema(&[REPO, ("title", "string", "Issue title", true), ("body", "string", "Issue description (markdown)", false), ("labels", "string[]", "Labels to apply", false)]),
            Level::Write,
        ),
        tool(
            "gitlab_add_comment",
            "Comment on an issue or a merge request. They are numbered separately, so say which.",
            schema(&[
                REPO,
                ("on", "string", "issue or merge_request", true),
                ("number", "integer", "Issue or merge request number", true),
                ("body", "string", "Comment (markdown)", true),
            ]),
            Level::Write,
        ),
        tool(
            "gitlab_create_merge_request",
            "Open a merge request from a pushed branch. Push the branch with git first.",
            schema(&[
                REPO,
                ("title", "string", "Merge request title", true),
                ("body", "string", "Description (markdown): what changed, why, how it was verified", false),
                ("head", "string", "Source branch, with the changes. Defaults to this session's work branch.", false),
                ("base", "string", "Target branch. Defaults to the project's default branch.", false),
                ("draft", "boolean", "Open as a draft", false),
            ]),
            Level::OpenPr,
        ),
        tool(
            "gitlab_merge_merge_request",
            "Merge a merge request.",
            schema(&[
                REPO,
                ("number", "integer", "Merge request number", true),
                ("method", "string", "merge or squash. Defaults to what the project and the merge request are set to.", false),
            ]),
            Level::MergePr,
        ),
    ]
}

/// GitLab says `opened` where people often say `open`.
fn state_arg<'a>(args: &'a Value, allowed: &[&str]) -> Result<&'a str, ToolError> {
    let state = match arg_opt(args, "state").unwrap_or("opened") {
        "open" => "opened",
        other => other,
    };
    if !allowed.contains(&state) {
        return Err(ToolError::Failed(format!("state must be one of: {}", allowed.join(", "))));
    }
    Ok(state)
}

/// Draft merge requests are marked in their title.
fn is_draft(mr: &Value) -> bool {
    mr["draft"] == true || mr["work_in_progress"] == true
}

pub async fn call(app: &App, session: &Session, name: &str, args: &Value) -> ToolResult {
    let repo = session_repo(app, session, args, Host::Gitlab).await?;
    let get = async |path: String| app.gitlab_api(Method::GET, &repo, &path, None).await;
    match name {
        "gitlab_list_issues" => {
            let state = state_arg(args, &["opened", "closed", "all"])?;
            let mut path = format!("/issues?state={state}&per_page=50");
            if let Some(labels) = arg_opt(args, "labels") {
                path.push_str(&format!("&labels={}", url::form_urlencoded::byte_serialize(labels.as_bytes()).collect::<String>()));
            }
            let issues = get(path).await?;
            let lines: Vec<String> = issues
                .as_array()
                .into_iter()
                .flatten()
                .map(|issue| {
                    let labels: Vec<&str> = issue["labels"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                    format!("#{} [{}] {} ({}){}", issue["iid"], s(issue, "/state"), s(issue, "/title"), s(issue, "/author/username"),
                        if labels.is_empty() { String::new() } else { format!(" labels: {}", labels.join(", ")) })
                })
                .collect();
            let text = if lines.is_empty() { format!("No {state} issues in {repo}.") } else { lines.join("\n") };
            Ok(ToolOutput::new(repo, text))
        }
        "gitlab_get_issue" => {
            let number = arg_u64(args, "number")?;
            let issue = get(format!("/issues/{number}")).await?;
            let notes = get(format!("/issues/{number}/notes?sort=asc&order_by=created_at&per_page=50")).await?;
            let mut text = format!(
                "#{number} {} [{}] by {}\n{}\n\n{}\n",
                s(&issue, "/title"), s(&issue, "/state"), s(&issue, "/author/username"), s(&issue, "/web_url"),
                clip(s(&issue, "/description"), 12_000)
            );
            // System notes record label changes and the like, not conversation.
            for note in notes.as_array().into_iter().flatten().filter(|note| note["system"] != true) {
                text.push_str(&format!("\n--- {} at {}\n{}\n", s(note, "/author/username"), s(note, "/created_at"), clip(s(note, "/body"), 4_000)));
            }
            Ok(ToolOutput::new(format!("{repo}#{number}"), text))
        }
        "gitlab_list_merge_requests" => {
            let state = state_arg(args, &["opened", "closed", "merged", "all"])?;
            let mrs = get(format!("/merge_requests?state={state}&per_page=50")).await?;
            let lines: Vec<String> = mrs
                .as_array()
                .into_iter()
                .flatten()
                .map(|mr| {
                    format!("!{} [{}{}] {} ({} → {}) by {}", mr["iid"], s(mr, "/state"), if is_draft(mr) { ", draft" } else { "" },
                        s(mr, "/title"), s(mr, "/source_branch"), s(mr, "/target_branch"), s(mr, "/author/username"))
                })
                .collect();
            let text = if lines.is_empty() { format!("No {state} merge requests in {repo}.") } else { lines.join("\n") };
            Ok(ToolOutput::new(repo, text))
        }
        "gitlab_get_merge_request" => {
            let number = arg_u64(args, "number")?;
            let mr = get(format!("/merge_requests/{number}")).await?;
            let diffs = get(format!("/merge_requests/{number}/diffs?per_page=100")).await?;
            let notes = get(format!("/merge_requests/{number}/notes?sort=asc&order_by=created_at&per_page=100")).await?;
            let mut text = format!(
                "!{number} {} [{}{}] {} → {}\n{}\nmerge status: {}{} · {} changed files\n\n{}\n\nFiles:\n",
                s(&mr, "/title"), s(&mr, "/state"),
                if is_draft(&mr) { ", draft" } else { "" },
                s(&mr, "/source_branch"), s(&mr, "/target_branch"), s(&mr, "/web_url"),
                mr["detailed_merge_status"].as_str().unwrap_or(s(&mr, "/merge_status")),
                if mr["has_conflicts"] == true { ", has conflicts" } else { "" },
                mr["changes_count"].as_str().unwrap_or("?"),
                clip(s(&mr, "/description"), 8_000)
            );
            for diff in diffs.as_array().into_iter().flatten() {
                let (old, new) = (s(diff, "/old_path"), s(diff, "/new_path"));
                text.push_str(&match () {
                    _ if diff["new_file"] == true => format!("  added {new}\n"),
                    _ if diff["deleted_file"] == true => format!("  removed {old}\n"),
                    _ if diff["renamed_file"] == true => format!("  renamed {old} → {new}\n"),
                    _ => format!("  modified {new}\n"),
                });
            }
            // Best effort: approvals are not on every plan or token.
            if let Ok(approvals) = get(format!("/merge_requests/{number}/approvals")).await {
                let by: Vec<&str> = approvals["approved_by"].as_array().into_iter().flatten().filter_map(|a| a["user"]["username"].as_str()).collect();
                if !by.is_empty() {
                    text.push_str(&format!("\nApproved by: {}\n", by.join(", ")));
                }
            }
            for note in notes.as_array().into_iter().flatten().filter(|note| note["system"] != true) {
                let place = match note.pointer("/position/new_path").and_then(Value::as_str) {
                    Some(path) => {
                        let line = note.pointer("/position/new_line").filter(|l| !l.is_null()).or_else(|| note.pointer("/position/old_line"));
                        format!(" on {path}:{}", line.cloned().unwrap_or(Value::Null))
                    }
                    None => format!(" at {}", s(note, "/created_at")),
                };
                text.push_str(&format!("\n--- {}{place}\n{}\n", s(note, "/author/username"), clip(s(note, "/body"), 3_000)));
            }
            if let Some(pipeline) = mr.get("head_pipeline").filter(|p| p.is_object()) {
                text.push_str(&format!("\nPipeline: {} {}\n", s(pipeline, "/status"), s(pipeline, "/web_url")));
                if let Ok(jobs) = get(format!("/pipelines/{}/jobs?per_page=50", pipeline["id"])).await {
                    for job in jobs.as_array().into_iter().flatten() {
                        text.push_str(&format!("  {}: {}\n", s(job, "/name"), s(job, "/status")));
                    }
                }
            }
            Ok(ToolOutput::new(format!("{repo}!{number}"), text))
        }
        "gitlab_create_issue" => {
            let title = arg_str(args, "title")?;
            let mut body = json!({ "title": title, "description": arg_opt(args, "body").unwrap_or("") });
            let labels = arg_list(args, "labels");
            if !labels.is_empty() {
                body["labels"] = json!(labels.join(","));
            }
            let issue = app.gitlab_api(Method::POST, &repo, "/issues", Some(body)).await?;
            let link = Link { kind: "issue".into(), title: format!("{repo}#{} {title}", issue["iid"]), url: s(&issue, "/web_url").to_string() };
            attach_link(app, session, &link).await;
            Ok(ToolOutput::new(format!("{repo}#{}", issue["iid"]), format!("Created issue #{}: {}", issue["iid"], link.url)))
        }
        "gitlab_add_comment" => {
            let (collection, mark) = match arg_str(args, "on")? {
                "issue" => ("issues", '#'),
                "merge_request" => ("merge_requests", '!'),
                _ => return Err(ToolError::Failed("`on` must be issue or merge_request".into())),
            };
            let number = arg_u64(args, "number")?;
            let body = arg_str(args, "body")?;
            let note = app.gitlab_api(Method::POST, &repo, &format!("/{collection}/{number}/notes"), Some(json!({ "body": body }))).await?;
            Ok(ToolOutput::new(format!("{repo}{mark}{number}"), format!("Commented on {mark}{number} (note {})", note["id"])))
        }
        "gitlab_create_merge_request" => {
            let title = arg_str(args, "title")?;
            let head = arg_opt(args, "head").unwrap_or(&session.work_branch).to_string();
            let default_branch = store::repo_by_name(&app.db, &repo).await?.map(|r| r.default_branch).unwrap_or_else(|| "main".into());
            let base = arg_opt(args, "base").unwrap_or(&default_branch).to_string();
            let author = store::user_by_id(&app.db, session.created_by).await?.map(|u| u.name).unwrap_or_default();
            let description = format!(
                "{}\n\n---\n_Opened by a [Hyperbeam]({}/sessions/{}) agent on behalf of {author}._",
                arg_opt(args, "body").unwrap_or(""),
                app.cfg.public_url.trim_end_matches('/'),
                session.id
            );
            let draft = args.get("draft").and_then(Value::as_bool).unwrap_or(false);
            let sent_title = if draft && !title.to_lowercase().starts_with("draft:") { format!("Draft: {title}") } else { title.to_string() };
            let payload = json!({ "title": sent_title, "description": description, "source_branch": head, "target_branch": base });
            let mr = app.gitlab_api(Method::POST, &repo, "/merge_requests", Some(payload)).await?;
            let number = mr["iid"].clone();
            // The board treats every host's reviewed change as a pull request.
            let link = Link { kind: "pull_request".into(), title: format!("{repo}!{number} {title}"), url: s(&mr, "/web_url").to_string() };
            attach_link(app, session, &link).await;
            app.activity(None, Some(session), None, "pr.opened", &format!("“{}” opened merge request {repo}!{number}: {title}", session.title), json!({ "url": link.url }))
                .await;
            Ok(ToolOutput::new(format!("{repo}!{number}"), format!("Opened merge request !{number}: {}", link.url)))
        }
        "gitlab_merge_merge_request" => {
            let number = arg_u64(args, "number")?;
            let payload = match arg_opt(args, "method") {
                None => json!({}),
                Some("merge") => json!({ "squash": false }),
                Some("squash") => json!({ "squash": true }),
                Some(_) => return Err(ToolError::Failed("method must be merge or squash".into())),
            };
            let merged = app.gitlab_api(Method::PUT, &repo, &format!("/merge_requests/{number}/merge"), Some(payload)).await?;
            app.activity(None, Some(session), None, "pr.merged", &format!("“{}” merged merge request {repo}!{number}", session.title), json!({}))
                .await;
            let sha = merged["merge_commit_sha"].as_str().or(merged["squash_commit_sha"].as_str()).or(merged["sha"].as_str()).unwrap_or("");
            Ok(ToolOutput::new(format!("{repo}!{number}"), format!("Merged !{number}: {sha}")))
        }
        other => Err(ToolError::Failed(format!("no tool named `{other}`"))),
    }
}
