//! FSRS owns intervals; the application owns their conversion into UTC deadlines.
//! Keep the model (including its learned decay) and retention together so grading,
//! rescheduling, and restoring state use the same calculation. Historical reviews
//! are never rewritten when the current scheduling policy changes.

use chrono::{DateTime, Utc};
use fsrs::{DEFAULT_PARAMETERS, FSRS};
use sqlx::{Row, SqliteConnection, SqlitePool};

use super::time::parse_flexible_datetime;
use crate::error::AppError;

pub(crate) struct Scheduler {
    pub model: FSRS,
    pub retention: f32,
}

impl Scheduler {
    pub fn new(parameters: Option<&str>, retention: f64) -> Result<Self, AppError> {
        let parameters: Vec<f32> = match parameters {
            Some(json) => serde_json::from_str(json)
                .map_err(|e| AppError::BadRequest(format!("Invalid FSRS parameters: {e}")))?,
            None => DEFAULT_PARAMETERS.to_vec(),
        };
        let model = FSRS::new(Some(&parameters))
            .map_err(|e| AppError::BadRequest(format!("Invalid FSRS parameters: {e:?}")))?;
        if !(0.5..=0.99).contains(&retention) {
            return Err(AppError::BadRequest("Invalid desired retention".into()));
        }
        let scheduler = Self {
            model,
            retention: retention as f32,
        };
        // The crate accepts legacy parameter lengths and upgrades them itself.
        // Also reject configurations whose forgetting curve has no usable interval.
        due_at(
            Utc::now(),
            scheduler
                .model
                .next_interval(Some(1.0), scheduler.retention, 3),
        )?;
        Ok(scheduler)
    }

    pub async fn load(conn: &mut SqliteConnection, user_id: i64) -> Result<Self, AppError> {
        let parameters: Option<String> =
            sqlx::query_scalar("SELECT parameters FROM users_fsrs_parameters WHERE user_id = ?")
                .bind(user_id)
                .fetch_optional(&mut *conn)
                .await?;
        let retention: Option<f64> =
            sqlx::query_scalar("SELECT desired_retention FROM users_settings WHERE user_id = ?")
                .bind(user_id)
                .fetch_optional(&mut *conn)
                .await?;
        Self::new(parameters.as_deref(), retention.unwrap_or(0.9))
    }

    pub fn due_at(&self, last_review: DateTime<Utc>, stability: f64) -> Result<i64, AppError> {
        if !stability.is_finite() || stability <= 0.0 {
            return Err(AppError::BadRequest(
                "Reviewed card must have positive finite stability".into(),
            ));
        }
        // With Some(stability), rating is ignored by this API. Do not call
        // next_states here: rescheduling is not another review.
        due_at(
            last_review,
            self.model
                .next_interval(Some(stability as f32), self.retention, 3),
        )
    }
}

/// Raw fractional days from fsrs 5.2.0, with no whole-day rounding, learning
/// steps, fuzz, or maximum-interval policy. Round the absolute deadline up to
/// the next UTC second so our second-resolution API never schedules early.
pub(crate) fn due_at(last_review: DateTime<Utc>, interval_days: f32) -> Result<i64, AppError> {
    let seconds = (last_review.timestamp() as f64
        + f64::from(last_review.timestamp_subsec_nanos()) / 1_000_000_000.0
        + f64::from(interval_days) * 86_400.0)
        .ceil();
    // Keep deadlines representable by SQLite strftime and the API's four-digit
    // UTC years (0001..9999); reject overflow instead of silently returning NULL.
    if !interval_days.is_finite()
        || interval_days <= 0.0
        || !(-62_135_596_800.0..=253_402_300_799.0).contains(&seconds)
    {
        return Err(AppError::BadRequest(
            "FSRS produced an invalid review interval".into(),
        ));
    }
    Ok(seconds as i64)
}

/// Call in the same write transaction as the settings/parameter/import change.
/// This deliberately preserves stability, difficulty, last_review and revisions.
pub(crate) async fn reschedule_user(
    conn: &mut SqliteConnection,
    user_id: i64,
) -> Result<(), AppError> {
    let scheduler = Scheduler::load(conn, user_id).await?;
    let rows = sqlx::query("SELECT card_id, stability, last_review FROM cards_states WHERE user_id = ? AND last_review IS NOT NULL")
        .bind(user_id).fetch_all(&mut *conn).await?;
    for row in rows {
        let last_review: String = row.get("last_review");
        let reviewed_at = parse_flexible_datetime(&last_review)
            .map_err(|e| AppError::BadRequest(format!("Invalid last review timestamp: {e}")))?;
        let deadline = scheduler.due_at(reviewed_at, row.get("stability"))?;
        sqlx::query("UPDATE cards_states SET due_at = ? WHERE user_id = ? AND card_id = ?")
            .bind(deadline)
            .bind(user_id)
            .bind(row.get::<i64, _>("card_id"))
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Complete the SQL migration with the real Rust model, including each user's
/// trained parameters. NULL is a migration marker only for reviewed cards;
/// new-card/reset tombstones intentionally have no deadline. Restart-safe.
pub(crate) async fn backfill_due_dates(pool: &SqlitePool) -> Result<(), AppError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let users: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT user_id FROM cards_states WHERE last_review IS NOT NULL AND due_at IS NULL",
    ).fetch_all(&mut *tx).await?;
    for user_id in users {
        reschedule_user(&mut tx, user_id).await.map_err(|error| {
            AppError::Internal(format!(
                "Cannot initialize due dates for user {user_id}: {error}"
            ))
        })?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/cards/scheduling.rs"]
mod tests;
