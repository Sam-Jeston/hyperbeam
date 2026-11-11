use super::{ToolDef, ToolError, ToolOutput, ToolResult, arg_list, arg_opt, arg_str, arg_u64, schema, session_repo};
use crate::app::App;
use crate::integrations::github::Access;
use crate::store::{self, Link, Session};
use hyperbeam_core::Grant;
use reqwest::Method;
use serde_json::{Value, json};

const REPO: (&str, &str, &str, bool) =
    ("repo", "string", "Repository as owner/name. Optional when the session has exactly one repository.", false);

pub fn tools() -> Vec<ToolDef> {
    let tool = |name, description, schema, grant| ToolDef { name, description, schema, grant: Some(grant), conductor_only: false };
    vec![
        tool(
            "github_list_issues",
            "List issues in a repository.",
            schema(&[REPO, ("state", "string", "open (default), closed or all", false), ("labels", "string", "Comma-separated label names", false)]),
            Grant::GITHUB_READ,
        ),
        tool(
            "github_get_issue",
            "Read an issue or pull request conversation: title, body and comments.",
            schema(&[REPO, ("number", "integer", "Issue or pull request number", true)]),
            Grant::GITHUB_READ,
        ),
        tool(
            "github_list_pull_requests",
            "List pull requests in a repository.",
            schema(&[REPO, ("state", "string", "open (default), closed or all", false)]),
            Grant::GITHUB_READ,
        ),
        tool(
            "github_get_pull_request",
            "Read a pull request: description, changed files, reviews, review comments and CI status.",
            schema(&[REPO, ("number", "integer", "Pull request number", true)]),
            Grant::GITHUB_READ,
        ),
        tool(
            "github_create_issue",
            "Open a new issue.",
            schema(&[REPO, ("title", "string", "Issue title", true), ("body", "string", "Issue body (markdown)", false), ("labels", "string[]", "Labels to apply", false)]),
            Grant::GITHUB_WRITE,
        ),
        tool(
            "github_add_comment",
            "Comment on an issue or pull request.",
            schema(&[REPO, ("number", "integer", "Issue or pull request number", true), ("body", "string", "Comment (markdown)", true)]),
            Grant::GITHUB_WRITE,
        ),
        tool(
            "github_create_pull_request",
            "Open a pull request from a pushed branch. Push the branch with git first.",
            schema(&[
                REPO,
                ("title", "string", "Pull request title", true),
                ("body", "string", "Description (markdown): what changed, why, how it was verified", false),
                ("head", "string", "Branch with the changes. Defaults to this session's work branch.", false),
                ("base", "string", "Branch to merge into. Defaults to the repository's default branch.", false),
                ("draft", "boolean", "Open as a draft", false),
            ]),
            Grant::GITHUB_OPEN_PR,
        ),
        tool(
            "github_merge_pull_request",
            "Merge a pull request.",
            schema(&[REPO, ("number", "integer", "Pull request number", true), ("method", "string", "merge, squash (default) or rebase", false)]),
            Grant::GITHUB_MERGE_PR,
        ),
    ]
}

fn s<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value.pointer(pointer).and_then(Value::as_str).unwrap_or("")
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}… [truncated]", text.chars().take(max).collect::<String>())
    }
}

/// Record a link on the session and its workstream, so the team can find it.
async fn attach_link(app: &App, session: &Session, link: &Link) {
    let value = json!([link]);
    sqlx::query("UPDATE sessions SET links = links || $2 WHERE id = $1 AND NOT links @> $2")
        .bind(session.id)
        .bind(&value)
        .execute(&app.db)
        .await
        .ok();
    app.publish_session(session.id).await;
    if let Some(ws) = session.workstream_id {
        sqlx::query("UPDATE workstreams SET links = links || $2, updated_at = now() WHERE id = $1 AND NOT links @> $2")
            .bind(ws)
            .bind(&value)
            .execute(&app.db)
            .await
            .ok();
        if link.kind == "pull_request" {
            // A pull request means people are up next.
            sqlx::query("UPDATE workstreams SET status = 'review' WHERE id = $1 AND status IN ('scoping', 'building')")
                .bind(ws)
                .execute(&app.db)
                .await
                .ok();
        }
        app.publish("workstream", None, json!({ "id": ws }));
    }
}

pub async fn call(app: &App, session: &Session, name: &str, args: &Value) -> ToolResult {
    let repo = session_repo(session, args)?;
    let get = async |path: String| app.github_api(Method::GET, &repo, &path, Access::Read, None).await;
    match name {
        "github_list_issues" => {
            let state = arg_opt(args, "state").unwrap_or("open");
            let mut path = format!("/issues?state={state}&per_page=50");
            if let Some(labels) = arg_opt(args, "labels") {
                path.push_str(&format!("&labels={}", labels.replace(' ', "%20")));
            }
            let issues = app.github_api(Method::GET, &repo, &path, Access::Read, None).await?;
            let lines: Vec<String> = issues
                .as_array()
                .into_iter()
                .flatten()
                .filter(|issue| issue.get("pull_request").is_none())
                .map(|issue| {
                    let labels: Vec<&str> = issue["labels"].as_array().into_iter().flatten().filter_map(|l| l["name"].as_str()).collect();
                    format!("#{} [{}] {} ({}){}", issue["number"], s(issue, "/state"), s(issue, "/title"), s(issue, "/user/login"),
                        if labels.is_empty() { String::new() } else { format!(" labels: {}", labels.join(", ")) })
                })
                .collect();
            let text = if lines.is_empty() { format!("No {state} issues in {repo}.") } else { lines.join("\n") };
            Ok(ToolOutput::new(repo, text))
        }
        "github_get_issue" => {
            let number = arg_u64(args, "number")?;
            let issue = app.github_api(Method::GET, &repo, &format!("/issues/{number}"), Access::Read, None).await?;
            let comments = app.github_api(Method::GET, &repo, &format!("/issues/{number}/comments?per_page=50"), Access::Read, None).await?;
            let mut text = format!(
                "#{number} {} [{}] by {}\n{}\n\n{}\n",
                s(&issue, "/title"), s(&issue, "/state"), s(&issue, "/user/login"), s(&issue, "/html_url"),
                clip(s(&issue, "/body"), 12_000)
            );
            for comment in comments.as_array().into_iter().flatten() {
                text.push_str(&format!("\n--- {} at {}\n{}\n", s(comment, "/user/login"), s(comment, "/created_at"), clip(s(comment, "/body"), 4_000)));
            }
            Ok(ToolOutput::new(format!("{repo}#{number}"), text))
        }
        "github_list_pull_requests" => {
            let state = arg_opt(args, "state").unwrap_or("open");
            let prs = app.github_api(Method::GET, &repo, &format!("/pulls?state={state}&per_page=50"), Access::Read, None).await?;
            let lines: Vec<String> = prs
                .as_array()
                .into_iter()
                .flatten()
                .map(|pr| {
                    format!("#{} [{}{}] {} ({} → {}) by {}", pr["number"], s(pr, "/state"),
                        if pr["draft"] == true { ", draft" } else { "" }, s(pr, "/title"), s(pr, "/head/ref"), s(pr, "/base/ref"), s(pr, "/user/login"))
                })
                .collect();
            let text = if lines.is_empty() { format!("No {state} pull requests in {repo}.") } else { lines.join("\n") };
            Ok(ToolOutput::new(repo, text))
        }
        "github_get_pull_request" => {
            let number = arg_u64(args, "number")?;
            let pr = get(format!("/pulls/{number}")).await?;
            let files = get(format!("/pulls/{number}/files?per_page=100")).await?;
            let reviews = get(format!("/pulls/{number}/reviews?per_page=50")).await?;
            let review_comments = get(format!("/pulls/{number}/comments?per_page=50")).await?;
            let mut text = format!(
                "#{number} {} [{}{}{}] {} → {}\n{}\nmergeable: {} · +{} −{} in {} files\n\n{}\n\nFiles:\n",
                s(&pr, "/title"), s(&pr, "/state"),
                if pr["draft"] == true { ", draft" } else { "" },
                if pr["merged"] == true { ", merged" } else { "" },
                s(&pr, "/head/ref"), s(&pr, "/base/ref"), s(&pr, "/html_url"),
                pr["mergeable"], pr["additions"], pr["deletions"], pr["changed_files"],
                clip(s(&pr, "/body"), 8_000)
            );
            for file in files.as_array().into_iter().flatten() {
                text.push_str(&format!("  {} {} (+{} −{})\n", s(file, "/status"), s(file, "/filename"), file["additions"], file["deletions"]));
            }
            for review in reviews.as_array().into_iter().flatten() {
                text.push_str(&format!("\n--- review by {}: {}\n{}\n", s(review, "/user/login"), s(review, "/state"), clip(s(review, "/body"), 3_000)));
            }
            for comment in review_comments.as_array().into_iter().flatten() {
                text.push_str(&format!("\n--- {} on {}:{}\n{}\n", s(comment, "/user/login"), s(comment, "/path"), comment["line"], clip(s(comment, "/body"), 2_000)));
            }
            let sha = s(&pr, "/head/sha").to_string();
            if !sha.is_empty() {
                // Best effort: tokens without the Checks permission cannot read this.
                if let Ok(checks) = get(format!("/commits/{sha}/check-runs?per_page=50")).await {
                    let runs: Vec<String> = checks["check_runs"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|run| format!("  {}: {}", s(run, "/name"), run["conclusion"].as_str().unwrap_or(s(run, "/status"))))
                        .collect();
                    if !runs.is_empty() {
                        text.push_str(&format!("\nCI checks:\n{}\n", runs.join("\n")));
                    }
                }
            }
            Ok(ToolOutput::new(format!("{repo}#{number}"), text))
        }
        "github_create_issue" => {
            let title = arg_str(args, "title")?;
            let mut body = json!({ "title": title, "body": arg_opt(args, "body").unwrap_or("") });
            let labels = arg_list(args, "labels");
            if !labels.is_empty() {
                body["labels"] = json!(labels);
            }
            let issue = app.github_api(Method::POST, &repo, "/issues", Access::Write, Some(body)).await?;
            let link = Link { kind: "issue".into(), title: format!("{repo}#{} {title}", issue["number"]), url: s(&issue, "/html_url").to_string() };
            attach_link(app, session, &link).await;
            Ok(ToolOutput::new(format!("{repo}#{}", issue["number"]), format!("Created issue #{}: {}", issue["number"], link.url)))
        }
        "github_add_comment" => {
            let number = arg_u64(args, "number")?;
            let body = arg_str(args, "body")?;
            let comment = app
                .github_api(Method::POST, &repo, &format!("/issues/{number}/comments"), Access::Write, Some(json!({ "body": body })))
                .await?;
            Ok(ToolOutput::new(format!("{repo}#{number}"), format!("Commented: {}", s(&comment, "/html_url"))))
        }
        "github_create_pull_request" => {
            let title = arg_str(args, "title")?;
            let head = arg_opt(args, "head").unwrap_or(&session.work_branch).to_string();
            let default_branch = store::repo_by_name(&app.db, &repo).await?.map(|r| r.default_branch).unwrap_or_else(|| "main".into());
            let base = arg_opt(args, "base").unwrap_or(&default_branch).to_string();
            let author = store::user_by_id(&app.db, session.created_by).await?.map(|u| u.name).unwrap_or_default();
            let body = format!(
                "{}\n\n---\n_Opened by a [Hyperbeam]({}/sessions/{}) agent on behalf of {author}._",
                arg_opt(args, "body").unwrap_or(""),
                app.cfg.public_url.trim_end_matches('/'),
                session.id
            );
            let payload = json!({ "title": title, "body": body, "head": head, "base": base, "draft": args.get("draft").and_then(Value::as_bool).unwrap_or(false) });
            let pr = app.github_api(Method::POST, &repo, "/pulls", Access::Write, Some(payload)).await?;
            let number = pr["number"].clone();
            let link = Link { kind: "pull_request".into(), title: format!("{repo}#{number} {title}"), url: s(&pr, "/html_url").to_string() };
            attach_link(app, session, &link).await;
            app.activity(None, Some(session), None, "pr.opened", &format!("“{}” opened pull request {repo}#{number}: {title}", session.title), json!({ "url": link.url }))
                .await;
            Ok(ToolOutput::new(format!("{repo}#{number}"), format!("Opened pull request #{number}: {}", link.url)))
        }
        "github_merge_pull_request" => {
            let number = arg_u64(args, "number")?;
            let method = arg_opt(args, "method").unwrap_or("squash");
            if !["merge", "squash", "rebase"].contains(&method) {
                return Err(ToolError::Failed("method must be merge, squash or rebase".into()));
            }
            let merged = app
                .github_api(Method::PUT, &repo, &format!("/pulls/{number}/merge"), Access::Write, Some(json!({ "merge_method": method })))
                .await?;
            app.activity(None, Some(session), None, "pr.merged", &format!("“{}” merged pull request {repo}#{number}", session.title), json!({}))
                .await;
            Ok(ToolOutput::new(format!("{repo}#{number}"), format!("Merged #{number} ({method}): {}", s(&merged, "/sha"))))
        }
        other => Err(ToolError::Failed(format!("no tool named `{other}`"))),
    }
}
