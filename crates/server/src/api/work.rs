use crate::app::App;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::sessions::{NewSession, can_drive};
use crate::store::{self, Event, Link, Session, User, Workflow, Workstream};
use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use hyperbeam_core::GrantSet;
use hyperbeam_core::protocol::SessionStatus;
use hyperbeam_core::workflow::{SessionRole, WorkflowSpec};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

const WORKSTREAM_STATUSES: [&str; 5] = ["scoping", "building", "review", "done", "archived"];

// --------------------------------------------------------------- workstreams

#[derive(Deserialize)]
pub struct ListWorkstreams {
    #[serde(default)]
    include_archived: bool,
}

pub async fn list_workstreams(State(app): State<App>, _user: AuthUser, Query(q): Query<ListWorkstreams>) -> ApiResult<Json<Value>> {
    let workstreams: Vec<Workstream> =
        sqlx::query_as("SELECT * FROM workstreams WHERE $1 OR status <> 'archived' ORDER BY updated_at DESC LIMIT 500")
            .bind(q.include_archived)
            .fetch_all(&app.db)
            .await?;
    Ok(Json(json!({ "workstreams": workstreams })))
}

#[derive(Deserialize)]
pub struct NewWorkstream {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    repos: Vec<String>,
}

pub async fn create_workstream(State(app): State<App>, AuthUser(user): AuthUser, Json(body): Json<NewWorkstream>) -> ApiResult<Json<Value>> {
    let workstream = app.open_workstream(&user, &body.title, &body.description, &body.repos).await?;
    Ok(Json(json!({ "workstream": workstream })))
}

pub async fn get_workstream(State(app): State<App>, _user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let workstream: Workstream =
        sqlx::query_as("SELECT * FROM workstreams WHERE id = $1").bind(id).fetch_optional(&app.db).await?.ok_or(ApiError::NotFound("workstream"))?;
    let sessions: Vec<Session> = sqlx::query_as(&format!(
        "SELECT {} FROM sessions WHERE workstream_id = $1 ORDER BY created_at",
        store::SESSION_COLUMNS
    ))
    .bind(id)
    .fetch_all(&app.db)
    .await?;
    let comments: Vec<(Uuid, Uuid, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT c.id, c.user_id, u.name, c.body, c.created_at FROM comments c JOIN users u ON u.id = c.user_id
         WHERE c.workstream_id = $1 ORDER BY c.created_at",
    )
    .bind(id)
    .fetch_all(&app.db)
    .await?;
    let comments: Vec<Value> = comments
        .into_iter()
        .map(|(id, user_id, author, body, created_at)| json!({ "id": id, "user_id": user_id, "author": author, "body": body, "created_at": created_at }))
        .collect();
    Ok(Json(json!({ "workstream": workstream, "sessions": sessions, "comments": comments })))
}

#[derive(Deserialize)]
pub struct UpdateWorkstream {
    title: Option<String>,
    description: Option<String>,
    status: Option<String>,
    links: Option<Vec<Link>>,
}

pub async fn update_workstream(
    State(app): State<App>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateWorkstream>,
) -> ApiResult<Json<Value>> {
    let current: Workstream =
        sqlx::query_as("SELECT * FROM workstreams WHERE id = $1").bind(id).fetch_optional(&app.db).await?.ok_or(ApiError::NotFound("workstream"))?;
    if let Some(status) = &body.status
        && !WORKSTREAM_STATUSES.contains(&status.as_str()) {
            return Err(ApiError::bad("unknown workstream status"));
        }
    if let Some(links) = &body.links
        && links.iter().any(|l| !(l.url.starts_with("https://") || l.url.starts_with("http://"))) {
            return Err(ApiError::bad("links must be http(s) URLs"));
        }
    let title = body.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(&current.title).to_string();
    let workstream: Workstream = sqlx::query_as(
        "UPDATE workstreams SET title = $2, description = $3, status = $4, links = $5, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(&title)
    .bind(body.description.as_deref().unwrap_or(&current.description))
    .bind(body.status.as_deref().unwrap_or(&current.status))
    .bind(json!(body.links.as_ref().unwrap_or(&current.links.0)))
    .fetch_one(&app.db)
    .await?;
    if workstream.status != current.status {
        app.activity(
            Some(user.id),
            None,
            Some(id),
            "workstream.status",
            &format!("{} moved “{title}” to {}", user.name, workstream.status),
            json!({ "status": workstream.status }),
        )
        .await;
    }
    app.publish("workstream", None, serde_json::to_value(&workstream).unwrap_or_default());
    Ok(Json(json!({ "workstream": workstream })))
}

#[derive(Deserialize)]
pub struct NewComment {
    body: String,
}

pub async fn add_comment(State(app): State<App>, AuthUser(user): AuthUser, Path(id): Path<Uuid>, Json(body): Json<NewComment>) -> ApiResult<Json<Value>> {
    let text = body.body.trim();
    if text.is_empty() {
        return Err(ApiError::bad("the comment is empty"));
    }
    let (title,): (String,) =
        sqlx::query_as("SELECT title FROM workstreams WHERE id = $1").bind(id).fetch_optional(&app.db).await?.ok_or(ApiError::NotFound("workstream"))?;
    let comment_id = Uuid::now_v7();
    sqlx::query("INSERT INTO comments (id, workstream_id, user_id, body) VALUES ($1, $2, $3, $4)")
        .bind(comment_id)
        .bind(id)
        .bind(user.id)
        .bind(text)
        .execute(&app.db)
        .await?;
    sqlx::query("UPDATE workstreams SET updated_at = now() WHERE id = $1").bind(id).execute(&app.db).await?;
    let preview: String = text.chars().take(140).collect();
    app.activity(Some(user.id), None, Some(id), "workstream.comment", &format!("{} commented on “{title}”: {preview}", user.name), json!({})).await;
    app.publish("workstream", None, json!({ "id": id }));
    Ok(Json(json!({ "id": comment_id })))
}

// ------------------------------------------------------------------ sessions

#[derive(Deserialize)]
pub struct ListSessions {
    workstream_id: Option<Uuid>,
    parent_id: Option<Uuid>,
    #[serde(default)]
    live: bool,
    #[serde(default)]
    mine: bool,
    limit: Option<i64>,
}

pub async fn list_sessions(State(app): State<App>, AuthUser(user): AuthUser, Query(q): Query<ListSessions>) -> ApiResult<Json<Value>> {
    let sessions: Vec<Session> = sqlx::query_as(&format!(
        "SELECT {} FROM sessions
         WHERE ($1::uuid IS NULL OR workstream_id = $1)
           AND ($2::uuid IS NULL OR parent_id = $2)
           AND (NOT $3 OR status IN ('pending', 'starting', 'running', 'idle'))
           AND ($4::uuid IS NULL OR created_by = $4)
         ORDER BY created_at DESC LIMIT $5",
        store::SESSION_COLUMNS
    ))
    .bind(q.workstream_id)
    .bind(q.parent_id)
    .bind(q.live)
    .bind(q.mine.then_some(user.id))
    .bind(q.limit.unwrap_or(200).clamp(1, 500))
    .fetch_all(&app.db)
    .await?;
    Ok(Json(json!({ "sessions": sessions })))
}

#[derive(Deserialize)]
pub struct NewSessionBody {
    title: Option<String>,
    prompt: String,
    workstream_id: Option<Uuid>,
    /// Open a new workstream with this title and put the session in it.
    new_workstream: Option<String>,
    #[serde(default)]
    role: SessionRole,
    harness: Option<String>,
    /// Model to run on. Omit for the harness profile's default.
    model: Option<String>,
    /// Environment to run in, by name. Omit for the repositories' default;
    /// `none` runs in the harness image.
    environment: Option<String>,
    #[serde(default)]
    repos: Vec<String>,
    #[serde(default)]
    skills: Vec<String>,
    /// Narrow the session below what you hold. Omit for everything you hold.
    grants: Option<Vec<String>>,
}

pub async fn create_session(State(app): State<App>, AuthUser(user): AuthUser, Json(body): Json<NewSessionBody>) -> ApiResult<Json<Value>> {
    let grants = match &body.grants {
        Some(list) => Some(GrantSet::parse_many(list).map_err(|e| ApiError::bad(e.to_string()))?),
        None => None,
    };
    if body.prompt.trim().is_empty() {
        return Err(ApiError::bad("a session needs a prompt"));
    }
    let workstream_id = match (&body.new_workstream, body.workstream_id) {
        (Some(title), _) if !title.trim().is_empty() => {
            let workstream = app.open_workstream(&user, title, &body.prompt, &body.repos).await?;
            if body.role == SessionRole::Worker {
                // A conductor scopes first; a single agent just starts building.
                sqlx::query("UPDATE workstreams SET status = 'building' WHERE id = $1").bind(workstream.id).execute(&app.db).await?;
            }
            Some(workstream.id)
        }
        (_, id) => id,
    };
    let session = app
        .create_session(
            &user,
            NewSession {
                title: body.title,
                prompt: body.prompt,
                workstream_id,
                parent: None,
                role: body.role,
                harness: body.harness,
                repos: body.repos,
                skills: body.skills,
                grants,
                workflow: None,
                model: body.model,
                environment: body.environment,
                context: None,
            },
        )
        .await?;
    Ok(Json(json!({ "session": session })))
}

#[derive(Deserialize)]
pub struct RunWorkflow {
    #[serde(default)]
    inputs: BTreeMap<String, String>,
    title: Option<String>,
    /// Overrides the workflow's model.
    model: Option<String>,
    /// Overrides the workflow's environment.
    environment: Option<String>,
    workstream_id: Option<Uuid>,
    new_workstream: Option<String>,
}

pub async fn run_workflow(State(app): State<App>, AuthUser(user): AuthUser, Path(id): Path<Uuid>, Json(body): Json<RunWorkflow>) -> ApiResult<Json<Value>> {
    let workflow: Workflow = sqlx::query_as("SELECT id, name, description, yaml, updated_at FROM workflows WHERE id = $1")
        .bind(id)
        .fetch_optional(&app.db)
        .await?
        .ok_or(ApiError::NotFound("workflow"))?;
    let spec = WorkflowSpec::from_yaml(&workflow.yaml).map_err(|e| ApiError::bad(e.to_string()))?;
    let prompt = spec.render(&body.inputs).map_err(|e| ApiError::bad(e.to_string()))?;
    let workstream_id = match (&body.new_workstream, body.workstream_id) {
        (Some(title), _) if !title.trim().is_empty() => Some(app.open_workstream(&user, title, &prompt, &spec.repos).await?.id),
        (_, id) => id,
    };
    let session = app
        .create_session(
            &user,
            NewSession {
                title: body.title.or_else(|| Some(format!("{}: {}", spec.name, prompt.lines().next().unwrap_or_default()))),
                prompt,
                workstream_id,
                parent: None,
                role: spec.role,
                harness: spec.harness.clone(),
                repos: spec.repos.clone(),
                skills: spec.skills.clone(),
                // A workflow with no grants listed asks for none.
                grants: Some(spec.grants.clone()),
                workflow: Some(spec.name.clone()),
                model: body.model.or_else(|| spec.model.clone()),
                environment: body.environment.or_else(|| spec.environment.clone()),
                context: None,
            },
        )
        .await?;
    Ok(Json(json!({ "session": session })))
}

async fn load_session(app: &App, id: Uuid) -> ApiResult<Session> {
    store::session_by_id(&app.db, id).await?.ok_or(ApiError::NotFound("session"))
}

pub async fn get_session(State(app): State<App>, AuthUser(user): AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let session = load_session(&app, id).await?;
    let harness = store::harness_by_id(&app.db, session.harness_id).await?;
    let children: Vec<Session> = sqlx::query_as(&format!("SELECT {} FROM sessions WHERE parent_id = $1 ORDER BY created_at", store::SESSION_COLUMNS))
        .bind(id)
        .fetch_all(&app.db)
        .await?;
    let parent: Option<(Uuid, String)> = match session.parent_id {
        Some(pid) => sqlx::query_as("SELECT id, title FROM sessions WHERE id = $1").bind(pid).fetch_optional(&app.db).await?,
        None => None,
    };
    let workstream: Option<(Uuid, String)> = match session.workstream_id {
        Some(ws) => sqlx::query_as("SELECT id, title FROM workstreams WHERE id = $1").bind(ws).fetch_optional(&app.db).await?,
        None => None,
    };
    let environment = match session.environment_id {
        Some(id) => store::environment_by_id(&app.db, id).await?,
        None => None,
    };
    Ok(Json(json!({
        "session": session,
        "environment": environment.map(|e| json!({ "name": e.name, "image": e.image })),
        "harness": harness.map(|h| {
            // What the session runs on, by its display name where known.
            let id = session.model.clone().or_else(|| h.default_model.clone());
            let label = id.as_ref().map(|id| h.choices().iter().find(|m| &m.id == id && !m.label.is_empty()).map(|m| m.label.clone()).unwrap_or_else(|| id.clone()));
            json!({ "name": h.name, "title": h.title, "model": label })
        }),
        "children": children,
        "parent": parent.map(|(id, title)| json!({ "id": id, "title": title })),
        "workstream": workstream.map(|(id, title)| json!({ "id": id, "title": title })),
        "can_drive": can_drive(&user, &session),
    })))
}

#[derive(Deserialize)]
pub struct ListEvents {
    #[serde(default)]
    after: i64,
    limit: Option<i64>,
}

pub async fn list_events(State(app): State<App>, _user: AuthUser, Path(id): Path<Uuid>, Query(q): Query<ListEvents>) -> ApiResult<Json<Value>> {
    load_session(&app, id).await?;
    let events: Vec<Event> = sqlx::query_as(
        "SELECT session_id, seq, ts, kind, data FROM session_events WHERE session_id = $1 AND seq > $2 ORDER BY seq LIMIT $3",
    )
    .bind(id)
    .bind(q.after)
    .bind(q.limit.unwrap_or(2000).clamp(1, 5000))
    .fetch_all(&app.db)
    .await?;
    Ok(Json(json!({ "events": events })))
}

fn require_drive(user: &User, session: &Session) -> ApiResult<()> {
    if can_drive(user, session) {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "this agent holds permissions you do not, so only its owner or an admin can direct it. You can still comment on its workstream.",
        ))
    }
}

#[derive(Deserialize)]
pub struct MessageBody {
    text: String,
}

pub async fn send_message(State(app): State<App>, AuthUser(user): AuthUser, Path(id): Path<Uuid>, Json(body): Json<MessageBody>) -> ApiResult<Json<Value>> {
    let session = load_session(&app, id).await?;
    require_drive(&user, &session)?;
    if !body.text.trim().is_empty() {
        // A person who writes to an agent that is waiting on a question has
        // seen it; whatever they say is the reply.
        app.clear_question(session.id, None).await?;
    }
    app.send_input(&session, Some(user.id), &user.name, &body.text).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct AnswerBody {
    text: String,
    /// The question being answered, so a stale page cannot answer a newer one.
    question_id: Option<Uuid>,
}

/// Answer the question an agent asked with `hyperbeam_ask_user`. Answering
/// directs the agent, so it takes the same standing as sending a message.
pub async fn answer_question(State(app): State<App>, AuthUser(user): AuthUser, Path(id): Path<Uuid>, Json(body): Json<AnswerBody>) -> ApiResult<Json<Value>> {
    let session = load_session(&app, id).await?;
    require_drive(&user, &session)?;
    app.answer_question(&session, &user, body.question_id, &body.text).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Interrupt the current turn; the session stays alive for a new prompt.
pub async fn cancel_session(State(app): State<App>, AuthUser(user): AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let session = load_session(&app, id).await?;
    require_drive(&user, &session)?;
    Ok(Json(json!({ "interrupted": app.cancel_turn(id) })))
}

/// Stop the session and tear down its sandbox.
pub async fn stop_session(State(app): State<App>, AuthUser(user): AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let session = load_session(&app, id).await?;
    require_drive(&user, &session)?;
    app.end_session(id, SessionStatus::Cancelled, None).await;
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------ activity/audit

#[derive(Deserialize)]
pub struct ListActivity {
    workstream_id: Option<Uuid>,
    before: Option<i64>,
    limit: Option<i64>,
}

pub async fn list_activity(State(app): State<App>, _user: AuthUser, Query(q): Query<ListActivity>) -> ApiResult<Json<Value>> {
    let rows: Vec<(i64, DateTime<Utc>, Option<Uuid>, Option<Uuid>, Option<Uuid>, String, String, Value)> = sqlx::query_as(
        "SELECT id, ts, user_id, session_id, workstream_id, kind, summary, data FROM activity
         WHERE ($1::uuid IS NULL OR workstream_id = $1) AND ($2::bigint IS NULL OR id < $2)
         ORDER BY id DESC LIMIT $3",
    )
    .bind(q.workstream_id)
    .bind(q.before)
    .bind(q.limit.unwrap_or(100).clamp(1, 500))
    .fetch_all(&app.db)
    .await?;
    let activity: Vec<Value> = rows
        .into_iter()
        .map(|(id, ts, user_id, session_id, workstream_id, kind, summary, data)| {
            json!({ "id": id, "ts": ts, "user_id": user_id, "session_id": session_id, "workstream_id": workstream_id, "kind": kind, "summary": summary, "data": data })
        })
        .collect();
    Ok(Json(json!({ "activity": activity })))
}

#[derive(Deserialize)]
pub struct ListAudit {
    session_id: Option<Uuid>,
    #[serde(default)]
    denied: bool,
    limit: Option<i64>,
}

pub async fn list_audit(State(app): State<App>, actor: AuthUser, Query(q): Query<ListAudit>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let rows: Vec<(i64, DateTime<Utc>, Option<Uuid>, Option<Uuid>, String, String, bool, String, Option<String>)> = sqlx::query_as(
        "SELECT a.id, a.ts, a.session_id, a.user_id, a.action, a.target, a.allowed, a.detail, s.title
         FROM audit a LEFT JOIN sessions s ON s.id = a.session_id
         WHERE ($1::uuid IS NULL OR a.session_id = $1) AND (NOT $2 OR NOT a.allowed)
         ORDER BY a.id DESC LIMIT $3",
    )
    .bind(q.session_id)
    .bind(q.denied)
    .bind(q.limit.unwrap_or(200).clamp(1, 1000))
    .fetch_all(&app.db)
    .await?;
    let audit: Vec<Value> = rows
        .into_iter()
        .map(|(id, ts, session_id, user_id, action, target, allowed, detail, session_title)| {
            json!({ "id": id, "ts": ts, "session_id": session_id, "user_id": user_id, "action": action, "target": target, "allowed": allowed, "detail": detail, "session_title": session_title })
        })
        .collect();
    Ok(Json(json!({ "audit": audit })))
}
