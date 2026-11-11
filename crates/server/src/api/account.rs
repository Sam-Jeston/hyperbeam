use super::nullable;
use crate::app::App;
use crate::auth::{self, AuthUser};
use crate::crypto;
use crate::error::{ApiError, ApiResult};
use crate::store::{self, User};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::header::SET_COOKIE;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use hyperbeam_core::protocol::SessionStatus;
use hyperbeam_core::{GrantSet, Role};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use uuid::Uuid;

fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else { return false };
    !local.is_empty() && domain.contains('.') && !email.contains(char::is_whitespace) && email.len() <= 254
}

fn check_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < 10 {
        return Err(ApiError::bad("choose a password of at least 10 characters"));
    }
    Ok(())
}

fn signed_in(app: &App, user: &User, token: &str) -> Response {
    ([(SET_COOKIE, auth::session_cookie(app, token))], Json(json!({ "user": user }))).into_response()
}

#[derive(Deserialize)]
pub struct SetupBody {
    team_name: String,
    name: String,
    email: String,
    password: String,
}

/// Create the team and its owner. Works exactly once.
pub async fn setup(State(app): State<App>, Json(body): Json<SetupBody>) -> ApiResult<Response> {
    let email = body.email.trim().to_lowercase();
    if !valid_email(&email) {
        return Err(ApiError::bad("enter a valid email address"));
    }
    if body.name.trim().is_empty() || body.team_name.trim().is_empty() {
        return Err(ApiError::bad("your name and a team name are required"));
    }
    check_password(&body.password)?;
    let hash = crypto::hash_password(&body.password)?;
    let user: Option<User> = sqlx::query_as(
        "INSERT INTO users (id, email, name, password_hash, role, grants)
         SELECT $1, $2, $3, $4, 'owner', $5 WHERE NOT EXISTS (SELECT 1 FROM users)
         RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(&email)
    .bind(body.name.trim())
    .bind(hash)
    // Everything built in. Connections are granted as they are added.
    .bind(json!(GrantSet::parse_many(["github:merge_pr", "github:write", "github:open_pr", "notion:write"]).expect("valid grants")))
    .fetch_optional(&app.db)
    .await?;
    let user = user.ok_or_else(|| ApiError::Conflict("this installation is already set up".into()))?;
    store::set_setting(&app.db, "team_name", &json!(body.team_name.trim())).await?;
    let token = auth::issue_session(&app, user.id).await?;
    Ok(signed_in(&app, &user, &token))
}

/// Failed sign-ins per email, to slow down guessing.
fn throttled(email: &str, failed: bool) -> bool {
    static ATTEMPTS: OnceLock<Mutex<HashMap<String, (u32, Instant)>>> = OnceLock::new();
    let mut attempts = ATTEMPTS.get_or_init(Default::default).lock().unwrap();
    let window = std::time::Duration::from_secs(15 * 60);
    attempts.retain(|_, (_, since)| since.elapsed() < window);
    let entry = attempts.entry(email.to_string()).or_insert((0, Instant::now()));
    if failed {
        entry.0 += 1;
    }
    entry.0 > 10
}

#[derive(Deserialize)]
pub struct LoginBody {
    email: String,
    password: String,
}

pub async fn login(State(app): State<App>, Json(body): Json<LoginBody>) -> ApiResult<Response> {
    let email = body.email.trim().to_lowercase();
    if throttled(&email, false) {
        return Err(ApiError::forbidden("too many failed attempts; try again in a few minutes"));
    }
    let user: Option<User> = sqlx::query_as("SELECT * FROM users WHERE email = $1 AND NOT disabled").bind(&email).fetch_optional(&app.db).await?;
    // Verify against something even when the account does not exist, so the
    // response time does not reveal which emails are registered.
    static DUMMY: OnceLock<String> = OnceLock::new();
    let dummy = DUMMY.get_or_init(|| crypto::hash_password("no such account").unwrap_or_default());
    let hash = user.as_ref().map(|u| u.password_hash.as_str()).unwrap_or(dummy);
    let ok = crypto::verify_password(&body.password, hash);
    match user {
        Some(user) if ok => {
            let token = auth::issue_session(&app, user.id).await?;
            Ok(signed_in(&app, &user, &token))
        }
        _ => {
            throttled(&email, true);
            Err(ApiError::bad("that email and password do not match"))
        }
    }
}

pub async fn logout(State(app): State<App>, parts: axum::http::request::Parts) -> ApiResult<Response> {
    let cookies = parts.headers.get(axum::http::header::COOKIE).and_then(|v| v.to_str().ok()).unwrap_or_default();
    for pair in cookies.split(';') {
        if let Some((name, value)) = pair.trim().split_once('=') {
            if name == auth::COOKIE_NAME {
                sqlx::query("DELETE FROM auth_tokens WHERE token_hash = $1 AND kind = 'session'")
                    .bind(crypto::hash_token(value))
                    .execute(&app.db)
                    .await?;
            }
        }
    }
    Ok(([(SET_COOKIE, auth::clear_cookie())], Json(json!({ "ok": true }))).into_response())
}

pub async fn me(AuthUser(user): AuthUser) -> Json<Value> {
    Json(json!({ "user": user }))
}

#[derive(Deserialize)]
pub struct UpdateMe {
    name: Option<String>,
    current_password: Option<String>,
    new_password: Option<String>,
}

pub async fn update_me(State(app): State<App>, AuthUser(user): AuthUser, Json(body): Json<UpdateMe>) -> ApiResult<Json<Value>> {
    if let Some(name) = body.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        sqlx::query("UPDATE users SET name = $2 WHERE id = $1").bind(user.id).bind(name).execute(&app.db).await?;
    }
    if let Some(new_password) = &body.new_password {
        if !crypto::verify_password(body.current_password.as_deref().unwrap_or_default(), &user.password_hash) {
            return Err(ApiError::bad("your current password is not right"));
        }
        check_password(new_password)?;
        sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
            .bind(user.id)
            .bind(crypto::hash_password(new_password)?)
            .execute(&app.db)
            .await?;
    }
    let user = store::user_by_id(&app.db, user.id).await?.ok_or(ApiError::Unauthorized)?;
    Ok(Json(json!({ "user": user })))
}

// ------------------------------------------------------------------ API keys

pub async fn list_api_keys(State(app): State<App>, AuthUser(user): AuthUser) -> ApiResult<Json<Value>> {
    let rows: Vec<(Uuid, String, DateTime<Utc>, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT id, name, created_at, last_used_at FROM auth_tokens WHERE user_id = $1 AND kind = 'api' ORDER BY created_at DESC",
    )
    .bind(user.id)
    .fetch_all(&app.db)
    .await?;
    let keys: Vec<Value> = rows
        .into_iter()
        .map(|(id, name, created_at, last_used_at)| json!({ "id": id, "name": name, "created_at": created_at, "last_used_at": last_used_at }))
        .collect();
    Ok(Json(json!({ "api_keys": keys })))
}

#[derive(Deserialize)]
pub struct NewApiKey {
    name: String,
}

pub async fn create_api_key(State(app): State<App>, AuthUser(user): AuthUser, Json(body): Json<NewApiKey>) -> ApiResult<Json<Value>> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad("give the key a name"));
    }
    let token = crypto::new_token("nmk");
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO auth_tokens (id, user_id, token_hash, kind, name) VALUES ($1, $2, $3, 'api', $4)")
        .bind(id)
        .bind(user.id)
        .bind(crypto::hash_token(&token))
        .bind(name)
        .execute(&app.db)
        .await?;
    // The only time the key itself is shown.
    Ok(Json(json!({ "id": id, "name": name, "token": token })))
}

pub async fn delete_api_key(State(app): State<App>, AuthUser(user): AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    sqlx::query("DELETE FROM auth_tokens WHERE id = $1 AND user_id = $2 AND kind = 'api'").bind(id).bind(user.id).execute(&app.db).await?;
    Ok(Json(json!({ "ok": true })))
}

// --------------------------------------------------------------------- users

pub async fn list_users(State(app): State<App>, _user: AuthUser) -> ApiResult<Json<Value>> {
    let users: Vec<User> = sqlx::query_as("SELECT * FROM users ORDER BY created_at").fetch_all(&app.db).await?;
    Ok(Json(json!({ "users": users })))
}

async fn check_repo_scope(app: &App, scope: &Option<Vec<String>>) -> ApiResult<Option<Value>> {
    let Some(list) = scope else { return Ok(None) };
    let mut names = Vec::new();
    for name in list {
        let repo = store::repo_by_name(&app.db, name).await?.ok_or_else(|| ApiError::bad(format!("repository `{name}` is not registered")))?;
        names.push(repo.full_name);
    }
    Ok(Some(json!(names)))
}

#[derive(Deserialize)]
pub struct UpdateUser {
    role: Option<Role>,
    grants: Option<Vec<String>>,
    #[serde(default, deserialize_with = "nullable")]
    repo_scope: Option<Option<Vec<String>>>,
    disabled: Option<bool>,
}

pub async fn update_user(
    State(app): State<App>,
    actor: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateUser>,
) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let target = store::user_by_id(&app.db, id).await?.ok_or(ApiError::NotFound("user"))?;
    let actor_role = actor.0.role();
    if target.role() == Role::Owner && actor_role != Role::Owner {
        return Err(ApiError::forbidden("only an owner can change an owner"));
    }
    if let Some(role) = body.role {
        if target.id == actor.0.id && role != target.role() {
            return Err(ApiError::bad("you cannot change your own role"));
        }
        if role == Role::Owner && actor_role != Role::Owner {
            return Err(ApiError::forbidden("only an owner can make someone an owner"));
        }
        sqlx::query("UPDATE users SET role = $2 WHERE id = $1").bind(id).bind(role.as_str()).execute(&app.db).await?;
    }
    if let Some(grants) = &body.grants {
        let set = GrantSet::parse_many(grants).map_err(|e| ApiError::bad(e.to_string()))?;
        super::connections::check_grants(&app, &set).await?;
        sqlx::query("UPDATE users SET grants = $2 WHERE id = $1").bind(id).bind(json!(set)).execute(&app.db).await?;
    }
    if let Some(scope) = &body.repo_scope {
        let value = check_repo_scope(&app, scope).await?;
        sqlx::query("UPDATE users SET repo_scope = $2 WHERE id = $1").bind(id).bind(value).execute(&app.db).await?;
    }
    if let Some(disabled) = body.disabled {
        if disabled && target.id == actor.0.id {
            return Err(ApiError::bad("you cannot remove your own access"));
        }
        sqlx::query("UPDATE users SET disabled = $2 WHERE id = $1").bind(id).bind(disabled).execute(&app.db).await?;
        if disabled {
            // Removing someone ends their sign-ins and their running agents.
            sqlx::query("DELETE FROM auth_tokens WHERE user_id = $1").bind(id).execute(&app.db).await?;
            let live: Vec<(Uuid,)> =
                sqlx::query_as("SELECT id FROM sessions WHERE created_by = $1 AND status IN ('pending','starting','running','idle')")
                    .bind(id)
                    .fetch_all(&app.db)
                    .await?;
            for (session_id,) in live {
                app.end_session(session_id, SessionStatus::Cancelled, Some("the session owner's access was removed")).await;
            }
        }
    }
    let user = store::user_by_id(&app.db, id).await?.ok_or(ApiError::NotFound("user"))?;
    Ok(Json(json!({ "user": user })))
}

// ------------------------------------------------------------------- invites

#[derive(Deserialize)]
pub struct NewInvite {
    email: String,
    #[serde(default)]
    role: Option<Role>,
    #[serde(default)]
    grants: Vec<String>,
    #[serde(default)]
    repo_scope: Option<Vec<String>>,
}

pub async fn create_invite(State(app): State<App>, actor: AuthUser, Json(body): Json<NewInvite>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let email = body.email.trim().to_lowercase();
    if !valid_email(&email) {
        return Err(ApiError::bad("enter a valid email address"));
    }
    let role = body.role.unwrap_or(Role::Member);
    if role == Role::Owner {
        return Err(ApiError::bad("invite as a member or admin; an owner can promote later"));
    }
    let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE email = $1").bind(&email).fetch_optional(&app.db).await?;
    if exists.is_some() {
        return Err(ApiError::Conflict(format!("{email} is already on the team")));
    }
    let grants = GrantSet::parse_many(&body.grants).map_err(|e| ApiError::bad(e.to_string()))?;
    super::connections::check_grants(&app, &grants).await?;
    let scope = check_repo_scope(&app, &body.repo_scope).await?;
    let token = crypto::new_token("nmi");
    let id = Uuid::now_v7();
    sqlx::query("DELETE FROM invites WHERE email = $1 AND accepted_at IS NULL").bind(&email).execute(&app.db).await?;
    sqlx::query(
        "INSERT INTO invites (id, email, role, grants, repo_scope, token_hash, created_by, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(&email)
    .bind(role.as_str())
    .bind(json!(grants))
    .bind(scope)
    .bind(crypto::hash_token(&token))
    .bind(actor.0.id)
    .bind(Utc::now() + Duration::days(7))
    .execute(&app.db)
    .await?;
    // Hyperbeam sends no email; the admin passes this link on. Shown once.
    let url = format!("{}/invite/{token}", app.cfg.public_url.trim_end_matches('/'));
    Ok(Json(json!({ "id": id, "email": email, "url": url })))
}

pub async fn list_invites(State(app): State<App>, actor: AuthUser) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    let rows: Vec<(Uuid, String, String, Value, Option<Value>, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, email, role, grants, repo_scope, created_at, expires_at FROM invites
         WHERE accepted_at IS NULL AND expires_at > now() ORDER BY created_at DESC",
    )
    .fetch_all(&app.db)
    .await?;
    let invites: Vec<Value> = rows
        .into_iter()
        .map(|(id, email, role, grants, repo_scope, created_at, expires_at)| {
            json!({ "id": id, "email": email, "role": role, "grants": grants, "repo_scope": repo_scope, "created_at": created_at, "expires_at": expires_at })
        })
        .collect();
    Ok(Json(json!({ "invites": invites })))
}

pub async fn delete_invite(State(app): State<App>, actor: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    actor.require_admin()?;
    sqlx::query("DELETE FROM invites WHERE id = $1 AND accepted_at IS NULL").bind(id).execute(&app.db).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Public: lets the invite page greet the person before they choose a password.
pub async fn invite_info(State(app): State<App>, Path(token): Path<String>) -> ApiResult<Json<Value>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT email FROM invites WHERE token_hash = $1 AND accepted_at IS NULL AND expires_at > now()")
        .bind(crypto::hash_token(&token))
        .fetch_optional(&app.db)
        .await?;
    let (email,) = row.ok_or(ApiError::NotFound("invite"))?;
    let team_name = store::get_setting(&app.db, "team_name").await?;
    Ok(Json(json!({ "email": email, "team_name": team_name })))
}

#[derive(Deserialize)]
pub struct AcceptInvite {
    token: String,
    name: String,
    password: String,
}

pub async fn accept_invite(State(app): State<App>, Json(body): Json<AcceptInvite>) -> ApiResult<Response> {
    if body.name.trim().is_empty() {
        return Err(ApiError::bad("your name is required"));
    }
    check_password(&body.password)?;
    let hash = crypto::hash_password(&body.password)?;
    let mut tx = app.db.begin().await?;
    // Claiming the invite and creating the user succeed or fail together.
    let invite: Option<(String, String, Value, Option<Value>)> = sqlx::query_as(
        "UPDATE invites SET accepted_at = now()
         WHERE token_hash = $1 AND accepted_at IS NULL AND expires_at > now()
         RETURNING email, role, grants, repo_scope",
    )
    .bind(crypto::hash_token(&body.token))
    .fetch_optional(&mut *tx)
    .await?;
    let (email, role, grants, repo_scope) = invite.ok_or_else(|| ApiError::bad("this invite link is no longer valid; ask for a new one"))?;
    let user: User = sqlx::query_as(
        "INSERT INTO users (id, email, name, password_hash, role, grants, repo_scope) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(&email)
    .bind(body.name.trim())
    .bind(hash)
    .bind(role)
    .bind(grants)
    .bind(repo_scope)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    app.activity(Some(user.id), None, None, "team.joined", &format!("{} joined the team", user.name), json!({})).await;
    let token = auth::issue_session(&app, user.id).await?;
    Ok(signed_in(&app, &user, &token))
}
