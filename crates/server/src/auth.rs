//! Request authentication: browser session cookie or bearer API key.

use crate::app::App;
use crate::crypto;
use crate::error::ApiError;
use crate::store::User;
use axum::extract::FromRequestParts;
use axum::http::header::{AUTHORIZATION, COOKIE};
use axum::http::request::Parts;
use chrono::{Duration, Utc};
use uuid::Uuid;

pub const COOKIE_NAME: &str = "noemune_session";
pub const SESSION_DAYS: i64 = 30;

pub fn bearer(parts: &Parts) -> Option<String> {
    let value = parts.headers.get(AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer "))?;
    Some(token.trim().to_string())
}

fn cookie(parts: &Parts) -> Option<String> {
    for header in parts.headers.get_all(COOKIE) {
        for pair in header.to_str().ok()?.split(';') {
            if let Some((name, value)) = pair.trim().split_once('=') {
                if name == COOKIE_NAME {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// The signed-in person making a request.
pub struct AuthUser(pub User);

impl AuthUser {
    pub fn require_admin(&self) -> Result<(), ApiError> {
        if self.0.is_admin() {
            Ok(())
        } else {
            Err(ApiError::forbidden("only team admins can do that"))
        }
    }
}

impl FromRequestParts<App> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let token = bearer(parts).or_else(|| cookie(parts)).ok_or(ApiError::Unauthorized)?;
        // Run tokens identify agents, never people.
        if token.starts_with("nmr_") {
            return Err(ApiError::Unauthorized);
        }
        let user: Option<User> = sqlx::query_as(
            "UPDATE auth_tokens t SET last_used_at = now()
             FROM users u
             WHERE t.token_hash = $1 AND u.id = t.user_id AND NOT u.disabled
               AND (t.expires_at IS NULL OR t.expires_at > now())
             RETURNING u.*",
        )
        .bind(crypto::hash_token(&token))
        .fetch_optional(&app.db)
        .await?;
        user.map(AuthUser).ok_or(ApiError::Unauthorized)
    }
}

/// Issue a browser session. Returns the token to set as a cookie.
pub async fn issue_session(app: &App, user_id: Uuid) -> Result<String, ApiError> {
    let token = crypto::new_token("nmu");
    sqlx::query("INSERT INTO auth_tokens (id, user_id, token_hash, kind, expires_at) VALUES ($1, $2, $3, 'session', $4)")
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(crypto::hash_token(&token))
        .bind(Utc::now() + Duration::days(SESSION_DAYS))
        .execute(&app.db)
        .await?;
    Ok(token)
}

pub fn session_cookie(app: &App, token: &str) -> String {
    let secure = if app.cfg.public_url.starts_with("https://") { "; Secure" } else { "" };
    format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
        SESSION_DAYS * 24 * 3600
    )
}

pub fn clear_cookie() -> String {
    format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}
