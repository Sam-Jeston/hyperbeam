use super::{ToolDef, ToolError, ToolOutput, ToolResult, arg_opt, arg_str, schema};
use crate::app::App;
use crate::store::Session;
use anyhow::{Context, Result, anyhow, bail};
use hyperbeam_core::Grant;
use reqwest::Method;
use serde_json::{Value, json};

pub fn tools() -> Vec<ToolDef> {
    let tool = |name, description, schema, grant| ToolDef { name, description, schema, grant: Some(grant), conductor_only: false };
    vec![
        tool(
            "notion_search",
            "Search Notion pages and databases shared with the Hyperbeam integration.",
            schema(&[("query", "string", "Text to search for", true)]),
            Grant::NotionRead,
        ),
        tool(
            "notion_get_page",
            "Read a Notion page as text.",
            schema(&[("page_id", "string", "Page id or URL", true)]),
            Grant::NotionRead,
        ),
        tool(
            "notion_create_page",
            "Create a Notion page under an existing page.",
            schema(&[
                ("parent_page_id", "string", "Parent page id or URL", true),
                ("title", "string", "Page title", true),
                ("content", "string", "Body as simple markdown (headings, lists, quotes, code fences)", false),
            ]),
            Grant::NotionWrite,
        ),
        tool(
            "notion_append_to_page",
            "Append content to the end of a Notion page.",
            schema(&[("page_id", "string", "Page id or URL", true), ("content", "string", "Simple markdown to append", true)]),
            Grant::NotionWrite,
        ),
    ]
}

async fn api(app: &App, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
    let token = app
        .integration_secret("notion")
        .await?
        .ok_or_else(|| anyhow!("Notion is not connected. An admin can connect it under Settings → Integrations."))?;
    let mut request = app
        .http
        .request(method, format!("{}{path}", app.cfg.notion_api_url.trim_end_matches('/')))
        .bearer_auth(token)
        .header("notion-version", "2022-06-28")
        .timeout(std::time::Duration::from_secs(30));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.context("could not reach Notion")?;
    let status = response.status();
    let body: Value = response.json().await.context("Notion sent an unreadable reply")?;
    if !status.is_success() {
        bail!("Notion returned {status}: {}", body["message"].as_str().unwrap_or("unknown error"));
    }
    Ok(body)
}

/// Accept a bare id, a dashed id, or a notion.so URL.
pub fn page_id(input: &str) -> Result<String, ToolError> {
    let tail = input.split(['?', '#']).next().unwrap_or(input).trim_end_matches('/');
    let tail = tail.rsplit('/').next().unwrap_or(tail);
    let hex: String = tail.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let compact: String = tail.chars().filter(|c| *c != '-').collect();
    // URLs end in `Title-Words-<32 hex>`; the id is the last 32 hex characters.
    let id = if compact.len() == 32 && compact.chars().all(|c| c.is_ascii_hexdigit()) {
        compact
    } else if hex.len() >= 32 && tail.len() >= 32 && tail[tail.len() - 32..].chars().all(|c| c.is_ascii_hexdigit()) {
        tail[tail.len() - 32..].to_string()
    } else {
        return Err(ToolError::Failed(format!("`{input}` is not a Notion page id or URL")));
    };
    Ok(format!("{}-{}-{}-{}-{}", &id[0..8], &id[8..12], &id[12..16], &id[16..20], &id[20..32]))
}

fn plain(rich: &Value) -> String {
    rich.as_array().into_iter().flatten().filter_map(|r| r["plain_text"].as_str()).collect()
}

fn title_of(page: &Value) -> String {
    if let Some(props) = page["properties"].as_object() {
        for prop in props.values() {
            if prop["type"] == "title" {
                return plain(&prop["title"]);
            }
        }
    }
    plain(&page["title"])
}

pub fn render_blocks(blocks: &[Value]) -> String {
    let mut out = String::new();
    for block in blocks {
        let kind = block["type"].as_str().unwrap_or_default();
        let text = plain(&block[kind]["rich_text"]);
        let line = match kind {
            "heading_1" => format!("# {text}"),
            "heading_2" => format!("## {text}"),
            "heading_3" => format!("### {text}"),
            "bulleted_list_item" => format!("- {text}"),
            "numbered_list_item" => format!("1. {text}"),
            "to_do" => format!("- [{}] {text}", if block[kind]["checked"] == true { "x" } else { " " }),
            "quote" | "callout" => format!("> {text}"),
            "code" => format!("```{}\n{text}\n```", block[kind]["language"].as_str().unwrap_or("")),
            "child_page" => format!("[child page: {}] ({})", block[kind]["title"].as_str().unwrap_or(""), block["id"].as_str().unwrap_or("")),
            "divider" => "---".to_string(),
            "paragraph" => text,
            other => format!("[{other} block]"),
        };
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn rich(text: &str) -> Value {
    // Notion caps one rich text item at 2000 characters.
    let chars: Vec<char> = text.chars().collect();
    Value::Array(chars.chunks(1900).map(|c| json!({ "type": "text", "text": { "content": c.iter().collect::<String>() } })).collect())
}

/// Simple markdown to Notion blocks.
pub fn to_blocks(markdown: &str) -> Vec<Value> {
    let block = |kind: &str, text: &str| json!({ "object": "block", "type": kind, kind: { "rich_text": rich(text) } });
    let mut blocks = Vec::new();
    let mut code: Option<(String, Vec<&str>)> = None;
    for line in markdown.lines() {
        if let Some((language, lines)) = &mut code {
            if line.trim_start().starts_with("```") {
                let language = if language.is_empty() { "plain text".to_string() } else { language.clone() };
                blocks.push(json!({ "object": "block", "type": "code", "code": { "rich_text": rich(&lines.join("\n")), "language": language } }));
                code = None;
            } else {
                lines.push(line);
            }
            continue;
        }
        let trimmed = line.trim();
        if let Some(language) = trimmed.strip_prefix("```") {
            code = Some((language.trim().to_string(), Vec::new()));
        } else if trimmed.is_empty() {
            continue;
        } else if let Some(text) = trimmed.strip_prefix("### ") {
            blocks.push(block("heading_3", text));
        } else if let Some(text) = trimmed.strip_prefix("## ") {
            blocks.push(block("heading_2", text));
        } else if let Some(text) = trimmed.strip_prefix("# ") {
            blocks.push(block("heading_1", text));
        } else if let Some(text) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            blocks.push(block("bulleted_list_item", text));
        } else if let Some(text) = trimmed.strip_prefix("> ") {
            blocks.push(block("quote", text));
        } else if let Some((number, text)) = trimmed.split_once(". ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) {
            let _ = number;
            blocks.push(block("numbered_list_item", text));
        } else {
            blocks.push(block("paragraph", trimmed));
        }
    }
    if let Some((_, lines)) = code {
        blocks.push(json!({ "object": "block", "type": "code", "code": { "rich_text": rich(&lines.join("\n")), "language": "plain text" } }));
    }
    blocks
}

async fn append(app: &App, page: &str, blocks: Vec<Value>) -> Result<()> {
    // At most 100 children per request.
    for chunk in blocks.chunks(100) {
        api(app, Method::PATCH, &format!("/blocks/{page}/children"), Some(json!({ "children": chunk }))).await?;
    }
    Ok(())
}

pub async fn call(app: &App, _session: &Session, name: &str, args: &Value) -> ToolResult {
    match name {
        "notion_search" => {
            let query = arg_str(args, "query")?;
            let data = api(app, Method::POST, "/search", Some(json!({ "query": query, "page_size": 15 }))).await?;
            let lines: Vec<String> = data["results"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| format!("[{}] {} — id {} {}", r["object"].as_str().unwrap_or(""), title_of(r), r["id"].as_str().unwrap_or(""), r["url"].as_str().unwrap_or("")))
                .collect();
            Ok(ToolOutput::new(query, if lines.is_empty() { "Nothing found. Pages must be shared with the Hyperbeam integration in Notion.".into() } else { lines.join("\n") }))
        }
        "notion_get_page" => {
            let id = page_id(arg_str(args, "page_id")?)?;
            let page = api(app, Method::GET, &format!("/pages/{id}"), None).await?;
            let mut blocks = Vec::new();
            let mut cursor: Option<String> = None;
            for _ in 0..10 {
                let mut path = format!("/blocks/{id}/children?page_size=100");
                if let Some(c) = &cursor {
                    path.push_str(&format!("&start_cursor={c}"));
                }
                let data = api(app, Method::GET, &path, None).await?;
                blocks.extend(data["results"].as_array().cloned().unwrap_or_default());
                cursor = data["next_cursor"].as_str().map(str::to_string);
                if cursor.is_none() {
                    break;
                }
            }
            let title = title_of(&page);
            Ok(ToolOutput::new(title.clone(), format!("# {title}\n{}\n\n{}", page["url"].as_str().unwrap_or(""), render_blocks(&blocks))))
        }
        "notion_create_page" => {
            let parent = page_id(arg_str(args, "parent_page_id")?)?;
            let title = arg_str(args, "title")?;
            let blocks = to_blocks(arg_opt(args, "content").unwrap_or(""));
            let (first, rest) = blocks.split_at(blocks.len().min(100));
            let page = api(
                app,
                Method::POST,
                "/pages",
                Some(json!({
                    "parent": { "page_id": parent },
                    "properties": { "title": { "title": rich(title) } },
                    "children": first,
                })),
            )
            .await?;
            let id = page["id"].as_str().unwrap_or_default().to_string();
            if !rest.is_empty() {
                append(app, &id, rest.to_vec()).await?;
            }
            Ok(ToolOutput::new(title, format!("Created page “{title}”: {} (id {id})", page["url"].as_str().unwrap_or(""))))
        }
        "notion_append_to_page" => {
            let id = page_id(arg_str(args, "page_id")?)?;
            let blocks = to_blocks(arg_str(args, "content")?);
            let count = blocks.len();
            append(app, &id, blocks).await?;
            Ok(ToolOutput::new(id, format!("Appended {count} block(s).")))
        }
        other => Err(ToolError::Failed(format!("no tool named `{other}`"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_ids_from_urls() {
        let want = "1429989f-e8ac-4eff-bc8f-57f56486db54";
        for input in [
            "1429989fe8ac4effbc8f57f56486db54",
            "1429989f-e8ac-4eff-bc8f-57f56486db54",
            "https://www.notion.so/acme/Launch-Plan-1429989fe8ac4effbc8f57f56486db54?pvs=4",
        ] {
            assert_eq!(page_id(input).ok().as_deref(), Some(want), "{input}");
        }
        assert!(page_id("not-an-id").is_err());
    }

    #[test]
    fn markdown_round_trip_shapes() {
        let blocks = to_blocks("# Title\n\ntext\n- a\n1. b\n> q\n```rust\nfn x() {}\n```\n");
        let kinds: Vec<&str> = blocks.iter().map(|b| b["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["heading_1", "paragraph", "bulleted_list_item", "numbered_list_item", "quote", "code"]);
        assert_eq!(blocks[5]["code"]["language"], "rust");
        assert_eq!(blocks[5]["code"]["rich_text"][0]["text"]["content"], "fn x() {}");
    }

    #[test]
    fn long_text_is_chunked() {
        let blocks = to_blocks(&"x".repeat(4000));
        assert_eq!(blocks[0]["paragraph"]["rich_text"].as_array().unwrap().len(), 3);
    }
}
