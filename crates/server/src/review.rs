//! Named agents and the implement-and-review loop of a workflow's conductor.
//!
//! The conductor still drives the loop: it decides what to tell the
//! implementer and when the work is ready to look at. The platform holds
//! what must not depend on its judgement: who may do what, that a reviewer's
//! conclusion is a verdict rather than prose, how many rounds there have
//! been, and that the loop stops when it runs out of them.

use crate::app::App;
use crate::error::{ApiError, ApiResult};
use crate::store::{Session, User};
use anyhow::Result;
use chrono::{DateTime, Utc};
use hyperbeam_core::workflow::{LoopSpec, MAX_LOOP_ROUNDS, Plan};
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::Json;
use uuid::Uuid;

pub const APPROVE: &str = "approve";
pub const REQUEST_CHANGES: &str = "request_changes";

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Verdict {
    pub id: Uuid,
    pub session_id: Uuid,
    pub agent: Option<String>,
    pub verdict: String,
    pub findings: String,
    pub created_at: DateTime<Utc>,
}

impl Verdict {
    pub fn approves(&self) -> bool {
        self.verdict == APPROVE
    }

    /// As a sentence fragment: "approved", "requested changes".
    pub fn past_tense(&self) -> &'static str {
        if self.approves() { "approved" } else { "requested changes" }
    }
}

const VERDICT_COLUMNS: &str = "id, session_id, agent, verdict, findings, created_at";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    /// Rounds remain and the reviewer has not approved.
    Running,
    /// The reviewer's latest verdict approves.
    Approved,
    /// Every round is spent and the latest verdict still asks for changes.
    Exhausted,
}

/// Where a conductor's loop has got to.
#[derive(Debug, Clone, Serialize)]
pub struct LoopState {
    pub implement: String,
    pub review: String,
    pub max_rounds: u32,
    /// Rounds finished: one for each verdict the reviewer has given.
    pub rounds: u32,
    pub standing: Standing,
    /// The reviewer's verdicts, oldest first; the nth ended round n.
    pub verdicts: Vec<Verdict>,
}

impl LoopState {
    pub fn covers(&self, agent: Option<&str>) -> bool {
        agent.is_some_and(|agent| agent == self.implement || agent == self.review)
    }

    pub fn exhausted(&self) -> bool {
        self.standing == Standing::Exhausted
    }

    /// Said to a conductor that tries to carry on past the last round.
    pub fn refusal(&self) -> String {
        format!(
            "this loop has used all {} of its rounds without `{}` approving, so the platform has stopped it. \
             Tell the people you work for what is still unresolved with `hyperbeam_ask_user` and end your turn; \
             they can allow more rounds",
            self.max_rounds, self.review
        )
    }
}

pub async fn plan_of(db: &PgPool, session_id: Uuid) -> sqlx::Result<Option<Plan>> {
    let row: Option<(Option<Json<Plan>>,)> = sqlx::query_as("SELECT plan FROM sessions WHERE id = $1").bind(session_id).fetch_optional(db).await?;
    Ok(row.and_then(|(plan,)| plan).map(|Json(plan)| plan))
}

fn state(review_loop: &LoopSpec, verdicts: Vec<Verdict>) -> LoopState {
    let rounds = verdicts.len() as u32;
    let standing = match verdicts.last() {
        Some(last) if last.approves() => Standing::Approved,
        _ if rounds >= review_loop.max_rounds => Standing::Exhausted,
        _ => Standing::Running,
    };
    LoopState {
        implement: review_loop.implement.clone(),
        review: review_loop.review.clone(),
        max_rounds: review_loop.max_rounds,
        rounds,
        standing,
        verdicts,
    }
}

/// The state of the loop a conductor runs, if its workflow declared one.
pub async fn loop_state(db: &PgPool, conductor_id: Uuid) -> sqlx::Result<Option<LoopState>> {
    let Some(review_loop) = plan_of(db, conductor_id).await?.and_then(|plan| plan.review_loop) else {
        return Ok(None);
    };
    let verdicts: Vec<Verdict> = sqlx::query_as(&format!(
        "SELECT {VERDICT_COLUMNS} FROM verdicts WHERE conductor_id = $1 AND agent = $2 ORDER BY created_at, id"
    ))
    .bind(conductor_id)
    .bind(&review_loop.review)
    .fetch_all(db)
    .await?;
    Ok(Some(state(&review_loop, verdicts)))
}

/// The loop a session works within: its own, or its conductor's.
pub async fn loop_around(db: &PgPool, session: &Session) -> sqlx::Result<Option<LoopState>> {
    match loop_state(db, session.id).await? {
        Some(found) => Ok(Some(found)),
        None => match session.parent_id {
            Some(parent_id) => loop_state(db, parent_id).await,
            None => Ok(None),
        },
    }
}

/// The latest verdict of each of these sessions that has given one.
pub async fn latest_verdicts(db: &PgPool, sessions: &[Uuid]) -> sqlx::Result<Vec<Verdict>> {
    sqlx::query_as(&format!(
        "SELECT DISTINCT ON (session_id) {VERDICT_COLUMNS} FROM verdicts WHERE session_id = ANY($1)
         ORDER BY session_id, created_at DESC, id DESC"
    ))
    .bind(sessions)
    .fetch_all(db)
    .await
}

impl App {
    /// Record a reviewing agent's verdict. Its conductor hears of it when the
    /// agent's turn ends, with its report; until then the agent may change
    /// its mind, and the later verdict replaces the earlier.
    pub async fn record_verdict(&self, session: &Session, verdict: &str, findings: &str) -> Result<Verdict> {
        let mut tx = self.db.begin().await?;
        sqlx::query("DELETE FROM verdicts WHERE session_id = $1 AND NOT delivered").bind(session.id).execute(&mut *tx).await?;
        let recorded: Verdict = sqlx::query_as(&format!(
            "INSERT INTO verdicts (id, session_id, conductor_id, agent, verdict, findings) VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING {VERDICT_COLUMNS}"
        ))
        .bind(Uuid::now_v7())
        .bind(session.id)
        .bind(session.parent_id)
        .bind(&session.agent)
        .bind(verdict)
        .bind(findings)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        self.activity(
            None,
            Some(session),
            None,
            "session.verdict",
            &format!("“{}” {}", session.title, recorded.past_tense()),
            json!({ "verdict": recorded.verdict }),
        )
        .await;
        self.publish_session(session.id).await;
        if let Some(parent_id) = session.parent_id {
            self.publish_session(parent_id).await;
        }
        Ok(recorded)
    }

    /// What to tell a conductor about a reviewing worker whose turn has just
    /// ended: the verdict it gave this turn and what the loop expects next,
    /// or that it gave none. Empty for a worker that owes no verdict.
    pub async fn verdict_report(&self, session: &Session, asking: bool) -> Result<String> {
        let given: Option<Verdict> = sqlx::query_as(&format!(
            "UPDATE verdicts SET delivered = true WHERE session_id = $1 AND NOT delivered RETURNING {VERDICT_COLUMNS}"
        ))
        .bind(session.id)
        .fetch_optional(&self.db)
        .await?;
        let Some(verdict) = given else {
            return Ok(match session.reports_verdict && !asking {
                true => "\n\nIt ended its turn without reporting a verdict. Ask it for one with `hyperbeam_message_agent`.".to_string(),
                false => String::new(),
            });
        };
        let in_loop = match session.parent_id {
            Some(parent_id) => loop_state(&self.db, parent_id).await?.filter(|state| session.agent.as_deref() == Some(state.review.as_str())),
            None => None,
        };
        let mut text = format!("\n\nIts verdict: {}", if verdict.approves() { "APPROVE" } else { "REQUEST CHANGES" });
        if let Some(state) = &in_loop {
            text.push_str(&format!(" (round {} of {})", state.rounds, state.max_rounds));
        }
        text.push_str(&format!(".\n\n{}", verdict.findings));
        let Some(state) = in_loop else { return Ok(text) };
        let fresh = plan_of(&self.db, session.parent_id.unwrap_or_default())
            .await?
            .and_then(|plan| plan.agents.get(&state.review).map(|agent| agent.fresh))
            .unwrap_or(false);
        text.push_str("\n\n");
        text.push_str(&match state.standing {
            Standing::Approved => "The loop is complete. Finish what remains of the request and report the outcome.".to_string(),
            Standing::Exhausted => format!(
                "That was the last of the loop's {} rounds and the work is not approved, so the platform will refuse to \
                 continue it. Tell the people you work for what is still unresolved with `hyperbeam_ask_user`, and end \
                 your turn. They can allow more rounds.",
                state.max_rounds
            ),
            Standing::Running => format!(
                "Next: send these findings to `{}` with `hyperbeam_message_agent`. When it reports back, {}.",
                state.implement,
                match fresh {
                    true => format!("start a new `{}` with `hyperbeam_spawn_agent`", state.review),
                    false => format!("ask this `{}` to look again with `hyperbeam_message_agent`", state.review),
                }
            ),
        });
        Ok(text)
    }

    /// A person lets a conductor's loop run for more rounds, and the
    /// conductor is told so it can pick the loop up again.
    pub async fn allow_rounds(&self, session: &Session, user: &User, more: u32) -> ApiResult<LoopState> {
        let state = loop_state(&self.db, session.id).await?.ok_or_else(|| ApiError::bad("this session does not run a loop"))?;
        let max_rounds = state.max_rounds.saturating_add(more);
        if more == 0 || max_rounds > MAX_LOOP_ROUNDS {
            return Err(ApiError::bad(format!("a loop runs for at most {MAX_LOOP_ROUNDS} rounds in all")));
        }
        sqlx::query("UPDATE sessions SET plan = jsonb_set(plan, '{loop,max_rounds}', to_jsonb($2::int)) WHERE id = $1")
            .bind(session.id)
            .bind(max_rounds as i32)
            .execute(&self.db)
            .await?;
        let rounds = if more == 1 { "1 more round".to_string() } else { format!("{more} more rounds") };
        self.activity(
            Some(user.id),
            Some(session),
            None,
            "session.rounds",
            &format!("{} allowed “{}” {rounds}", user.name, session.title),
            json!({ "max_rounds": max_rounds }),
        )
        .await;
        let text = match state.standing {
            Standing::Approved => format!("The loop may now run for {rounds}, up to {max_rounds} in all."),
            _ => format!(
                "The loop may now run for {rounds}, up to {max_rounds} in all. Carry on from `{}`'s latest findings.",
                state.review
            ),
        };
        self.send_input(session, Some(user.id), &user.name, &text).await?;
        Ok(loop_state(&self.db, session.id).await?.unwrap_or(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(verdict: &str) -> Verdict {
        Verdict { id: Uuid::nil(), session_id: Uuid::nil(), agent: None, verdict: verdict.into(), findings: String::new(), created_at: Utc::now() }
    }

    #[test]
    fn a_loop_ends_on_approval_or_when_its_rounds_run_out() {
        let spec = LoopSpec { implement: "implementer".into(), review: "reviewer".into(), max_rounds: 2 };
        assert_eq!(state(&spec, vec![]).standing, Standing::Running);
        assert_eq!(state(&spec, vec![verdict(REQUEST_CHANGES)]).standing, Standing::Running);
        assert_eq!(state(&spec, vec![verdict(APPROVE)]).standing, Standing::Approved);
        let spent = state(&spec, vec![verdict(REQUEST_CHANGES), verdict(REQUEST_CHANGES)]);
        assert_eq!((spent.rounds, spent.standing), (2, Standing::Exhausted));
        // Approval in the last round is still approval.
        assert_eq!(state(&spec, vec![verdict(REQUEST_CHANGES), verdict(APPROVE)]).standing, Standing::Approved);
        // A later look that finds a problem reopens the question.
        assert_eq!(state(&spec, vec![verdict(APPROVE), verdict(REQUEST_CHANGES)]).standing, Standing::Exhausted);
        assert!(spent.covers(Some("reviewer")) && !spent.covers(Some("docs")) && !spent.covers(None));
    }
}
