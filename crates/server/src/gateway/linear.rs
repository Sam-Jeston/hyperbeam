use super::{ToolDef, ToolError, ToolOutput, ToolResult, arg_opt, arg_str, schema};
use crate::app::App;
use crate::store::Session;
use anyhow::{Context, Result, anyhow, bail};
use hyperbeam_core::Grant;
use serde_json::{Value, json};

pub fn tools() -> Vec<ToolDef> {
    let tool = |name, description, schema, grant| ToolDef { name, description, schema, grant: Some(grant), conductor_only: false };
    vec![
        tool("linear_list_teams", "List Linear teams and their keys.", schema(&[]), Grant::LinearRead),
        tool(
            "linear_search_issues",
            "Search Linear issues by text, optionally within a team.",
            schema(&[
                ("query", "string", "Text to find in titles and descriptions", false),
                ("team", "string", "Team key, e.g. ENG", false),
                ("limit", "integer", "Maximum results (default 20)", false),
            ]),
            Grant::LinearRead,
        ),
        tool(
            "linear_get_issue",
            "Read a Linear issue with its description and comments.",
            schema(&[("id", "string", "Issue identifier, e.g. ENG-123", true)]),
            Grant::LinearRead,
        ),
        tool(
            "linear_create_issue",
            "Create a Linear issue.",
            schema(&[
                ("team", "string", "Team key, e.g. ENG", true),
                ("title", "string", "Issue title", true),
                ("description", "string", "Description (markdown)", false),
            ]),
            Grant::LinearWrite,
        ),
        tool(
            "linear_update_issue",
            "Update a Linear issue's title, description or workflow state.",
            schema(&[
                ("id", "string", "Issue identifier, e.g. ENG-123", true),
                ("title", "string", "New title", false),
                ("description", "string", "New description (markdown)", false),
                ("state", "string", "Workflow state name, e.g. In Progress", false),
            ]),
            Grant::LinearWrite,
        ),
        tool(
            "linear_add_comment",
            "Comment on a Linear issue.",
            schema(&[("id", "string", "Issue identifier, e.g. ENG-123", true), ("body", "string", "Comment (markdown)", true)]),
            Grant::LinearWrite,
        ),
    ]
}

async fn graphql(app: &App, query: &str, variables: Value) -> Result<Value> {
    let key = app
        .integration_secret("linear")
        .await?
        .ok_or_else(|| anyhow!("Linear is not connected. An admin can connect it under Settings → Integrations."))?;
    // Personal API keys are sent bare; OAuth access tokens as Bearer.
    let auth = if key.starts_with("lin_api_") { key } else { format!("Bearer {key}") };
    let response = app
        .http
        .post(&app.cfg.linear_api_url)
        .header("authorization", auth)
        .json(&json!({ "query": query, "variables": variables }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .context("could not reach Linear")?;
    let status = response.status();
    let body: Value = response.json().await.context("Linear sent an unreadable reply")?;
    if let Some(errors) = body.get("errors").and_then(Value::as_array) {
        let messages: Vec<&str> = errors.iter().filter_map(|e| e["message"].as_str()).collect();
        bail!("Linear: {}", messages.join("; "));
    }
    if !status.is_success() {
        bail!("Linear returned {status}");
    }
    Ok(body.get("data").cloned().unwrap_or(Value::Null))
}

fn s<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value.pointer(pointer).and_then(Value::as_str).unwrap_or("")
}

async fn issue_ids(app: &App, identifier: &str) -> Result<(String, String)> {
    let data = graphql(app, "query($id: String!) { issue(id: $id) { id team { id } } }", json!({ "id": identifier })).await?;
    let id = s(&data, "/issue/id");
    if id.is_empty() {
        bail!("no Linear issue `{identifier}`");
    }
    Ok((id.to_string(), s(&data, "/issue/team/id").to_string()))
}

pub async fn call(app: &App, _session: &Session, name: &str, args: &Value) -> ToolResult {
    match name {
        "linear_list_teams" => {
            let data = graphql(app, "{ teams(first: 100) { nodes { key name } } }", json!({})).await?;
            let lines: Vec<String> = data["teams"]["nodes"].as_array().into_iter().flatten().map(|t| format!("{} — {}", s(t, "/key"), s(t, "/name"))).collect();
            Ok(ToolOutput::new("teams", lines.join("\n")))
        }
        "linear_search_issues" => {
            let mut filter = json!({});
            if let Some(query) = arg_opt(args, "query") {
                filter["or"] = json!([{ "title": { "containsIgnoreCase": query } }, { "description": { "containsIgnoreCase": query } }]);
            }
            if let Some(team) = arg_opt(args, "team") {
                filter["team"] = json!({ "key": { "eq": team.to_uppercase() } });
            }
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20).clamp(1, 50);
            let data = graphql(
                app,
                "query($filter: IssueFilter, $first: Int) { issues(filter: $filter, first: $first, orderBy: updatedAt) {
                   nodes { identifier title url state { name } assignee { name } } } }",
                json!({ "filter": filter, "first": limit }),
            )
            .await?;
            let lines: Vec<String> = data["issues"]["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|i| {
                    let assignee = s(i, "/assignee/name");
                    format!("{} [{}] {}{} {}", s(i, "/identifier"), s(i, "/state/name"), s(i, "/title"),
                        if assignee.is_empty() { String::new() } else { format!(" ({assignee})") }, s(i, "/url"))
                })
                .collect();
            Ok(ToolOutput::new(arg_opt(args, "query").unwrap_or("issues"), if lines.is_empty() { "No matching issues.".into() } else { lines.join("\n") }))
        }
        "linear_get_issue" => {
            let id = arg_str(args, "id")?;
            let data = graphql(
                app,
                "query($id: String!) { issue(id: $id) { identifier title description url priorityLabel
                   state { name } assignee { name } labels { nodes { name } }
                   comments(first: 50) { nodes { body createdAt user { name } } } } }",
                json!({ "id": id }),
            )
            .await?;
            let issue = &data["issue"];
            if issue.is_null() {
                return Err(ToolError::Failed(format!("no Linear issue `{id}`")));
            }
            let labels: Vec<&str> = issue["labels"]["nodes"].as_array().into_iter().flatten().filter_map(|l| l["name"].as_str()).collect();
            let mut text = format!(
                "{} {} [{}]\n{}\nassignee: {} · priority: {} · labels: {}\n\n{}\n",
                s(issue, "/identifier"), s(issue, "/title"), s(issue, "/state/name"), s(issue, "/url"),
                s(issue, "/assignee/name"), s(issue, "/priorityLabel"), labels.join(", "), s(issue, "/description")
            );
            for comment in issue["comments"]["nodes"].as_array().into_iter().flatten() {
                text.push_str(&format!("\n--- {} at {}\n{}\n", s(comment, "/user/name"), s(comment, "/createdAt"), s(comment, "/body")));
            }
            Ok(ToolOutput::new(id, text))
        }
        "linear_create_issue" => {
            let team = arg_str(args, "team")?.to_uppercase();
            let title = arg_str(args, "title")?;
            let teams = graphql(app, "query($key: String!) { teams(filter: { key: { eq: $key } }) { nodes { id } } }", json!({ "key": team })).await?;
            let team_id = s(&teams, "/teams/nodes/0/id");
            if team_id.is_empty() {
                return Err(ToolError::Failed(format!("no Linear team with key `{team}`")));
            }
            let data = graphql(
                app,
                "mutation($input: IssueCreateInput!) { issueCreate(input: $input) { issue { identifier url } } }",
                json!({ "input": { "teamId": team_id, "title": title, "description": arg_opt(args, "description").unwrap_or("") } }),
            )
            .await?;
            let identifier = s(&data, "/issueCreate/issue/identifier").to_string();
            Ok(ToolOutput::new(identifier.clone(), format!("Created {identifier}: {}", s(&data, "/issueCreate/issue/url"))))
        }
        "linear_update_issue" => {
            let id = arg_str(args, "id")?;
            let (issue_id, team_id) = issue_ids(app, id).await?;
            let mut input = json!({});
            if let Some(title) = arg_opt(args, "title") {
                input["title"] = json!(title);
            }
            if let Some(description) = arg_opt(args, "description") {
                input["description"] = json!(description);
            }
            if let Some(state) = arg_opt(args, "state") {
                let states = graphql(
                    app,
                    "query($team: ID!) { workflowStates(filter: { team: { id: { eq: $team } } }) { nodes { id name } } }",
                    json!({ "team": team_id }),
                )
                .await?;
                let nodes = states["workflowStates"]["nodes"].as_array().cloned().unwrap_or_default();
                let Some(found) = nodes.iter().find(|n| s(n, "/name").eq_ignore_ascii_case(state)) else {
                    let names: Vec<&str> = nodes.iter().map(|n| s(n, "/name")).collect();
                    return Err(ToolError::Failed(format!("no state `{state}`; this team has: {}", names.join(", "))));
                };
                input["stateId"] = found["id"].clone();
            }
            if input.as_object().is_some_and(|o| o.is_empty()) {
                return Err(ToolError::Failed("nothing to update: pass title, description or state".into()));
            }
            graphql(
                app,
                "mutation($id: String!, $input: IssueUpdateInput!) { issueUpdate(id: $id, input: $input) { success } }",
                json!({ "id": issue_id, "input": input }),
            )
            .await?;
            Ok(ToolOutput::new(id, format!("Updated {id}.")))
        }
        "linear_add_comment" => {
            let id = arg_str(args, "id")?;
            let body = arg_str(args, "body")?;
            let (issue_id, _) = issue_ids(app, id).await?;
            graphql(
                app,
                "mutation($input: CommentCreateInput!) { commentCreate(input: $input) { success } }",
                json!({ "input": { "issueId": issue_id, "body": body } }),
            )
            .await?;
            Ok(ToolOutput::new(id, format!("Commented on {id}.")))
        }
        other => Err(ToolError::Failed(format!("no tool named `{other}`"))),
    }
}
