//! The knowledge base: documents written for people to read, such as
//! reports, investigations and designs.
//!
//! It differs from team notes on purpose. A note is a short standing fact
//! that agents overwrite as they learn; a document is a dated piece of
//! writing that stays tied to the session and workstream it came out of.
//!
//! A team keeps its documents in one of two places: here, in the `documents`
//! table, where the web app lists, searches and shows them; or in Notion,
//! under a page an admin chose, in which case Hyperbeam only holds the
//! setting and each document becomes a page there.

use crate::app::App;
use crate::connections::{self, Connection};
use crate::error::ApiError;
use crate::store::{self, Session};
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

pub const MAX_DOCUMENT_BYTES: usize = 200 * 1024;
const MAX_TITLE_CHARS: usize = 200;
const MAX_SUMMARY_CHARS: usize = 300;
/// The connection documents go through when the knowledge base is in Notion.
const NOTION: &str = "notion";
/// Notion's tools, as its hosted MCP server names them.
const NOTION_CREATE: &str = "notion-create-pages";
const NOTION_FETCH: &str = "notion-fetch";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Documents are kept in Hyperbeam's own database.
    #[default]
    Internal,
    /// Documents become pages in Notion.
    Notion,
}

/// The Notion page documents are created under.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotionTarget {
    pub page_id: String,
    pub url: String,
    /// The page's title when Notion told us; empty otherwise.
    #[serde(default)]
    pub title: String,
}

/// The `knowledge_base` setting.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub mode: Mode,
    /// Kept when the team switches back to internal, so switching again is one click.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notion: Option<NotionTarget>,
}

impl Config {
    pub async fn load(db: &PgPool) -> sqlx::Result<Self> {
        Ok(store::get_setting(db, "knowledge_base").await?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
    }

    /// Where in Notion documents go, when that is where they go.
    pub fn notion_target(&self) -> Option<&NotionTarget> {
        match self.mode {
            Mode::Notion => self.notion.as_ref(),
            Mode::Internal => None,
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Document {
    pub id: Uuid,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub session_id: Option<Uuid>,
    pub workstream_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A document without its body, for lists and search results.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Listed {
    pub id: Uuid,
    pub title: String,
    pub summary: String,
    pub session_id: Option<Uuid>,
    pub workstream_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const COLUMNS: &str = "id, title, summary, content, session_id, workstream_id, created_by, updated_by, created_at, updated_at";
const LISTED: &str = "id, title, summary, session_id, workstream_id, created_by, created_at, updated_at";

/// What a document must be before it is stored anywhere.
pub fn check(title: &str, content: &str) -> Result<(), String> {
    if title.trim().is_empty() {
        return Err("a document needs a title".into());
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(format!("the title is too long (limit {MAX_TITLE_CHARS} characters)"));
    }
    if content.trim().is_empty() {
        return Err("a document needs content".into());
    }
    if content.len() > MAX_DOCUMENT_BYTES {
        return Err(format!("the document is too large (limit {} KB); split it", MAX_DOCUMENT_BYTES / 1024));
    }
    Ok(())
}

/// The one-line summary: what was given, or else the document's first line of prose.
pub fn summary_of(given: Option<&str>, content: &str) -> String {
    let line = given.map(str::trim).filter(|s| !s.is_empty()).or_else(|| {
        content.lines().map(str::trim).find(|line| !line.is_empty() && !line.starts_with(['#', '`', '|', '-', '>', '*']))
    });
    let line = line.unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= MAX_SUMMARY_CHARS {
        line
    } else {
        format!("{}…", line.chars().take(MAX_SUMMARY_CHARS - 1).collect::<String>())
    }
}

pub async fn by_id(db: &PgPool, id: Uuid) -> sqlx::Result<Option<Document>> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM documents WHERE id = $1")).bind(id).fetch_optional(db).await
}

/// Documents matching `query`, best match first; the newest when it is empty.
pub async fn find(db: &PgPool, query: &str, workstream: Option<Uuid>, limit: i64) -> sqlx::Result<Vec<Listed>> {
    let query = query.trim();
    if query.is_empty() {
        return sqlx::query_as(&format!(
            "SELECT {LISTED} FROM documents WHERE ($1::uuid IS NULL OR workstream_id = $1) ORDER BY updated_at DESC LIMIT $2"
        ))
        .bind(workstream)
        .bind(limit)
        .fetch_all(db)
        .await;
    }
    // Whole words by meaning, and a title that merely contains what was typed,
    // so a half-typed word still finds something.
    sqlx::query_as(&format!(
        "SELECT {LISTED} FROM documents, websearch_to_tsquery('english', $1) AS q
         WHERE (search @@ q OR title ILIKE '%' || $1 || '%') AND ($2::uuid IS NULL OR workstream_id = $2)
         ORDER BY ts_rank(search, q) DESC, updated_at DESC LIMIT $3"
    ))
    .bind(query)
    .bind(workstream)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Take a deleted document's link off the sessions and workstreams that carried it.
pub async fn forget_link(db: &PgPool, url: &str) -> sqlx::Result<()> {
    for table in ["sessions", "workstreams"] {
        sqlx::query(&format!(
            "UPDATE {table} SET links = (SELECT coalesce(jsonb_agg(l), '[]'::jsonb) FROM jsonb_array_elements(links) AS l WHERE l->>'url' <> $1)
             WHERE links @> jsonb_build_array(jsonb_build_object('url', $1::text))"
        ))
        .bind(url)
        .execute(db)
        .await?;
    }
    Ok(())
}

/// The page id inside a Notion address or pasted id, in dashed form.
/// `https://www.notion.so/acme/Reports-1f0c…` and a bare id both work.
pub fn notion_page_id(input: &str) -> Option<String> {
    let path = input.trim().split(['?', '#']).next()?;
    let last = path.trim_end_matches('/').rsplit('/').next()?;
    let hex: Vec<char> = last.chars().filter(|c| *c != '-').collect();
    let tail = hex.len().checked_sub(32).map(|start| &hex[start..])?;
    if !tail.iter().all(char::is_ascii_hexdigit) {
        return None;
    }
    let part = |range: std::ops::Range<usize>| tail[range].iter().collect::<String>().to_lowercase();
    Some(format!("{}-{}-{}-{}-{}", part(0..8), part(8..12), part(12..16), part(16..20), part(20..32)))
}

/// The first `url` anywhere inside a reply from Notion.
fn url_in(value: &Value) -> Option<&str> {
    match value {
        Value::Object(map) => map
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
            .or_else(|| map.values().find_map(url_in)),
        Value::Array(items) => items.iter().find_map(url_in),
        _ => None,
    }
}

/// A document an agent just published, wherever it went.
pub struct Published {
    /// Set when it is kept in Hyperbeam, where it can be revised by id.
    pub id: Option<Uuid>,
    pub title: String,
    pub url: String,
    pub revised: bool,
    pub in_notion: bool,
}

impl App {
    /// The address of a document kept in Hyperbeam.
    pub fn document_url(&self, id: Uuid) -> String {
        format!("{}/knowledge/{id}", self.cfg.public_url.trim_end_matches('/'))
    }

    /// Whether Notion is connected and can create pages.
    pub async fn notion_ready(&self) -> sqlx::Result<bool> {
        Ok(connections::by_name(&self.db, NOTION).await?.is_some_and(|conn| conn.tools.iter().any(|t| t.name == NOTION_CREATE)))
    }

    async fn call_notion(&self, conn: &Connection, tool: &str, args: &Value) -> Result<String> {
        let full = conn.endpoint(self).await?;
        let result = match self.mcp.call_tool(&self.http, &full, tool, args).await {
            Ok(result) => result,
            // A token from the sign-in can lapse early; renew it and retry once.
            Err(err) => match self.renewed(conn, &full, &err).await? {
                Some(fresh) => self.mcp.call_tool(&self.http, &fresh, tool, args).await?,
                None => return Err(err),
            },
        };
        let text = result
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|c| c.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        if result.get("isError") == Some(&Value::Bool(true)) {
            return Err(anyhow!("{}", if text.is_empty() { "Notion refused the request" } else { text.as_str() }));
        }
        Ok(text)
    }

    /// Settle where in Notion documents go, from what an admin pasted. The
    /// second value is a caution worth showing: the page could not be opened.
    pub async fn notion_target(&self, input: &str) -> Result<(NotionTarget, Option<String>), ApiError> {
        let page_id = notion_page_id(input)
            .ok_or_else(|| ApiError::bad("that is not a Notion page: paste the page's link (Share → Copy link) or its id"))?;
        let conn = connections::by_name(&self.db, NOTION).await?.ok_or_else(|| ApiError::bad("connect Notion under Settings → Integrations first"))?;
        if !conn.tools.iter().any(|t| t.name == NOTION_CREATE) {
            return Err(ApiError::bad("the Notion connection does not offer a way to create pages; refresh its tools under Settings → Integrations"));
        }
        let pasted = input.trim();
        let url = if pasted.starts_with("https://") { pasted.to_string() } else { format!("https://www.notion.so/{}", page_id.replace('-', "")) };
        let mut target = NotionTarget { page_id, url, title: String::new() };
        // Opening the page proves the signed-in account can see it, and tells us what it is called.
        let mut caution = None;
        if conn.tools.iter().any(|t| t.name == NOTION_FETCH) {
            match self.call_notion(&conn, NOTION_FETCH, &json!({ "id": target.page_id })).await {
                Ok(text) => {
                    let title = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("title").and_then(Value::as_str).map(str::to_string));
                    target.title = title.unwrap_or_default().chars().take(MAX_TITLE_CHARS).collect();
                }
                Err(err) => {
                    let why: String = format!("{err:#}").chars().take(300).collect();
                    caution = Some(format!(
                        "Saved, but Notion could not open that page ({why}). Check that the account Hyperbeam is signed in to Notion with can see it."
                    ));
                }
            }
        }
        Ok((target, caution))
    }

    async fn publish_to_notion(&self, target: &NotionTarget, session: &Session, title: &str, summary: &str, content: &str) -> Result<String, ApiError> {
        let failed = |why: String| ApiError::bad(format!("the knowledge base is in Notion, and Notion did not take the page: {why}"));
        let conn = connections::by_name(&self.db, NOTION).await?.ok_or_else(|| failed("Notion is no longer connected. An admin can reconnect it under Settings → Integrations".into()))?;
        let author = store::user_by_id(&self.db, session.created_by).await?.map(|u| u.name).unwrap_or_else(|| "someone".into());
        let mut body = String::new();
        if !summary.is_empty() && !content.contains(summary) {
            body.push_str(&format!("{summary}\n\n"));
        }
        body.push_str(content.trim_end());
        body.push_str(&format!(
            "\n\n---\nWritten by the Hyperbeam agent “{}” for {author}: {}/s/{}\n",
            session.title,
            self.cfg.public_url.trim_end_matches('/'),
            session.id
        ));
        let args = json!({
            "parent": { "page_id": target.page_id },
            "pages": [{ "properties": { "title": title }, "content": body }],
        });
        let reply = self.call_notion(&conn, NOTION_CREATE, &args).await.map_err(|err| failed(format!("{err:#}")))?;
        // Notion answers with the pages it made. Without an address in the
        // reply, the place they were made under is the next best link.
        let url = serde_json::from_str::<Value>(&reply).ok().and_then(|v| url_in(&v).map(str::to_string));
        Ok(url.unwrap_or_else(|| target.url.clone()))
    }

    /// Publish a document on a session's behalf, to wherever the team keeps them.
    pub async fn publish_document(&self, session: &Session, title: &str, summary: Option<&str>, content: &str, revise: Option<Uuid>) -> Result<Published, ApiError> {
        check(title, content).map_err(ApiError::bad)?;
        let title = title.trim();
        let summary = summary_of(summary, content);
        let config = Config::load(&self.db).await?;
        if let Some(target) = config.notion_target() {
            if revise.is_some() {
                return Err(ApiError::bad("the knowledge base is in Notion, so there is no document with that id to revise; edit the page with the `notion_*` tools, or publish a new one"));
            }
            let url = self.publish_to_notion(target, session, title, &summary, content).await?;
            return Ok(Published { id: None, title: title.to_string(), url, revised: false, in_notion: true });
        }
        let id = match revise {
            Some(id) => {
                let existing = by_id(&self.db, id).await?.ok_or_else(|| ApiError::bad(format!("no document with id `{id}`")))?;
                // A session may rewrite what it, or its workstream, wrote: not the team's other records.
                let own = existing.session_id == Some(session.id) || (existing.workstream_id.is_some() && existing.workstream_id == session.workstream_id);
                if !own {
                    return Err(ApiError::forbidden("that document came out of other work; publish a new document rather than rewriting it"));
                }
                sqlx::query("UPDATE documents SET title = $2, summary = $3, content = $4, session_id = $5, updated_by = NULL, updated_at = now() WHERE id = $1")
                    .bind(id)
                    .bind(title)
                    .bind(&summary)
                    .bind(content)
                    .bind(session.id)
                    .execute(&self.db)
                    .await?;
                id
            }
            None => {
                let id = Uuid::now_v7();
                sqlx::query("INSERT INTO documents (id, title, summary, content, session_id, workstream_id, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7)")
                    .bind(id)
                    .bind(title)
                    .bind(&summary)
                    .bind(content)
                    .bind(session.id)
                    .bind(session.workstream_id)
                    .bind(session.created_by)
                    .execute(&self.db)
                    .await?;
                id
            }
        };
        Ok(Published { id: Some(id), title: title.to_string(), url: self.document_url(id), revised: revise.is_some(), in_notion: false })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notion_page_id_is_found_in_whatever_was_pasted() {
        let id = Some("1f0c2b3a-4d5e-6f70-8192-a3b4c5d6e7f8".to_string());
        assert_eq!(notion_page_id("https://www.notion.so/acme/Agent-reports-1f0c2b3a4d5e6f708192a3b4c5d6e7f8?pvs=4"), id);
        assert_eq!(notion_page_id("https://www.notion.so/1F0C2B3A4D5E6F708192A3B4C5D6E7F8/"), id);
        assert_eq!(notion_page_id(" 1f0c2b3a-4d5e-6f70-8192-a3b4c5d6e7f8 "), id);
        assert_eq!(notion_page_id("https://www.notion.so/acme/Agent-reports"), None);
        assert_eq!(notion_page_id(""), None);
    }

    #[test]
    fn a_summary_falls_back_to_the_first_line_of_prose() {
        assert_eq!(summary_of(Some("  Checkout is slow because of N+1 queries. "), "# Title\nBody"), "Checkout is slow because of N+1 queries.");
        assert_eq!(summary_of(None, "# Checkout latency\n\n- a list\n\nThe cart page  issues one query per line item.\nMore."), "The cart page issues one query per line item.");
        assert_eq!(summary_of(Some(""), "# Only a heading"), "");
        assert_eq!(summary_of(None, &"word ".repeat(200)).chars().count(), MAX_SUMMARY_CHARS);
    }

    #[test]
    fn the_address_of_a_new_page_is_read_from_notions_reply() {
        let reply = json!({ "pages": [{ "id": "p1", "url": "https://www.notion.so/Report-abc", "properties": { "title": "Report" } }] });
        assert_eq!(url_in(&reply), Some("https://www.notion.so/Report-abc"));
        assert_eq!(url_in(&json!({ "pages": [{ "id": "p1", "url": "not an address" }] })), None);
    }
}
