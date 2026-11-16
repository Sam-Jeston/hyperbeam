//! The knowledge base, for people: where documents go, and the documents
//! kept in Hyperbeam itself.

use crate::app::App;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::knowledge::{self, Config, Mode};
use crate::store;
use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

async fn describe(app: &App, caution: Option<String>) -> ApiResult<Json<Value>> {
    let (documents,): (i64,) = sqlx::query_as("SELECT count(*) FROM documents").fetch_one(&app.db).await?;
    Ok(Json(json!({
        "config": Config::load(&app.db).await?,
        // Whether choosing Notion is possible yet.
        "notion_connected": app.notion_ready().await?,
        // How many documents Hyperbeam itself holds, whatever the setting says now.
        "documents": documents,
        "caution": caution,
    })))
}

pub async fn get_config(State(app): State<App>, _user: AuthUser) -> ApiResult<Json<Value>> {
    describe(&app, None).await
}

#[derive(Deserialize)]
pub struct ConfigBody {
    mode: Mode,
    /// A Notion page's link or id: where documents go when the mode is `notion`.
    notion_page: Option<String>,
}

pub async fn put_config(State(app): State<App>, actor: AuthUser, Json(body): Json<ConfigBody>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let mut config = Config::load(&app.db).await?;
    let mut caution = None;
    let pasted = body.notion_page.as_deref().map(str::trim).filter(|page| !page.is_empty());
    match (body.mode, pasted) {
        // The same page as before needs no second look.
        (Mode::Notion, Some(page)) if config.notion.as_ref().is_none_or(|current| current.url != page && current.page_id != page) => {
            let (target, warned) = app.notion_target(page).await?;
            (config.notion, caution) = (Some(target), warned);
        }
        (Mode::Notion, None) if config.notion.is_none() => {
            return Err(ApiError::bad("choose the Notion page documents should be created under"));
        }
        (Mode::Notion, _) if !app.notion_ready().await? => {
            return Err(ApiError::bad("connect Notion under Settings → Integrations first"));
        }
        _ => {}
    }
    config.mode = body.mode;
    store::set_setting(&app.db, "knowledge_base", &json!(config)).await?;
    describe(&app, caution).await
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    q: String,
    workstream: Option<Uuid>,
}

pub async fn list(State(app): State<App>, _user: AuthUser, Query(query): Query<ListQuery>) -> ApiResult<Json<Value>> {
    let documents = knowledge::find(&app.db, &query.q, query.workstream, 200).await?;
    Ok(Json(json!({ "documents": documents })))
}

pub async fn get(State(app): State<App>, _user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let document = knowledge::by_id(&app.db, id).await?.ok_or(ApiError::NotFound("document"))?;
    Ok(Json(json!({ "document": document })))
}

#[derive(Deserialize)]
pub struct DocumentBody {
    title: String,
    #[serde(default)]
    summary: String,
    content: String,
}

pub async fn create(State(app): State<App>, actor: AuthUser, Json(body): Json<DocumentBody>) -> ApiResult<Json<Value>> {
    knowledge::check(&body.title, &body.content).map_err(ApiError::bad)?;
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO documents (id, title, summary, content, created_by) VALUES ($1, $2, $3, $4, $5)")
        .bind(id)
        .bind(body.title.trim())
        .bind(knowledge::summary_of(Some(&body.summary), &body.content))
        .bind(&body.content)
        .bind(actor.0.id)
        .execute(&app.db)
        .await?;
    get(State(app), actor, Path(id)).await
}

pub async fn update(State(app): State<App>, actor: AuthUser, Path(id): Path<Uuid>, Json(body): Json<DocumentBody>) -> ApiResult<Json<Value>> {
    knowledge::check(&body.title, &body.content).map_err(ApiError::bad)?;
    let changed = sqlx::query("UPDATE documents SET title = $2, summary = $3, content = $4, updated_by = $5, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(body.title.trim())
        .bind(knowledge::summary_of(Some(&body.summary), &body.content))
        .bind(&body.content)
        .bind(actor.0.id)
        .execute(&app.db)
        .await?;
    if changed.rows_affected() == 0 {
        return Err(ApiError::NotFound("document"));
    }
    get(State(app), actor, Path(id)).await
}

pub async fn delete(State(app): State<App>, _actor: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    sqlx::query("DELETE FROM documents WHERE id = $1").bind(id).execute(&app.db).await?;
    // The sessions and workstream that produced it no longer point at a missing page.
    knowledge::forget_link(&app.db, &app.document_url(id)).await?;
    Ok(Json(json!({ "ok": true })))
}
