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
mod tests {
    use super::*;
    use crate::test_support::{test_pool, test_user};
    use crate::{
        auth::AuthUser,
        cards,
        error::{AppJson, AppPath, AppQuery},
        export_import, user,
    };
    use axum::extract::State;
    use serde_json::{Value, json};

    #[test]
    fn intervals_invert_the_forgetting_curve_including_learned_and_legacy_decay() {
        // Independent check against the crate's forward forgetting curve, not
        // a second implementation of next_interval. Crate 5.2.0 models FSRS-6;
        // 17/19-parameter legacy models are upgraded with decay 0.5.
        let mut learned = DEFAULT_PARAMETERS.to_vec();
        learned[20] = 0.3;
        for (parameters, decay) in [
            (DEFAULT_PARAMETERS.to_vec(), DEFAULT_PARAMETERS[20]),
            (learned, 0.3),
            (DEFAULT_PARAMETERS[..19].to_vec(), 0.5),
            (DEFAULT_PARAMETERS[..17].to_vec(), 0.5),
        ] {
            let serialized = serde_json::to_string(&parameters).unwrap();
            for retention in [0.5, 0.8, 0.9, 0.95, 0.99] {
                let scheduler = Scheduler::new(Some(&serialized), retention).unwrap();
                for stability in [0.01, 1.0, 121.01552] {
                    let interval =
                        scheduler
                            .model
                            .next_interval(Some(stability), retention as f32, 3);
                    let predicted = fsrs::current_retrievability(
                        fsrs::MemoryState {
                            stability,
                            difficulty: 5.0,
                        },
                        interval,
                        decay,
                    );
                    assert!((predicted - retention as f32).abs() < 0.00001);
                    if retention == 0.9 {
                        assert!((interval / stability - 1.0).abs() < 0.00001);
                    } else if retention > 0.9 {
                        assert!(interval < stability);
                    } else {
                        assert!(interval > stability);
                    }
                }
            }
        }
    }

    #[test]
    fn deadlines_preserve_fractional_days_and_never_round_early() {
        let reviewed = DateTime::parse_from_rfc3339("2026-01-01T12:00:00.500Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            due_at(reviewed, 0.25).unwrap(),
            reviewed.timestamp() + 21_601
        );
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::MAX] {
            assert!(due_at(reviewed, bad).is_err());
        }
        assert!(Scheduler::new(Some("[1,2,3]"), 0.9).is_err());
        assert!(Scheduler::new(Some("garbage"), 0.9).is_err());
    }

    async fn target(pool: &SqlitePool) -> String {
        sqlx::query_scalar("SELECT form FROM targets WHERE sentence_id = (SELECT id FROM sentences WHERE card_id = 1 LIMIT 1)")
            .fetch_one(pool).await.unwrap()
    }

    async fn set_retention(pool: &SqlitePool, uid: i64, retention: f64) {
        let _ = user::update_settings(
            State(pool.clone()),
            AuthUser(uid),
            AppJson(
                serde_json::from_value(
                    json!({"desired_retention":retention,"daily_new_card_limit":0}),
                )
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    }

    async fn deadline(pool: &SqlitePool, uid: i64) -> i64 {
        sqlx::query_scalar("SELECT due_at FROM cards_states WHERE user_id = ? AND card_id = 1")
            .bind(uid)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn next(pool: &SqlitePool, uid: i64) -> Value {
        serde_json::to_value(
            cards::get_review_queue(
                State(pool.clone()),
                AuthUser(uid),
                AppQuery(Default::default()),
            )
            .await
            .unwrap()
            .0,
        )
        .unwrap()
    }

    async fn stats(pool: &SqlitePool, uid: i64) -> Value {
        serde_json::to_value(
            cards::get_stats(State(pool.clone()), AuthUser(uid))
                .await
                .unwrap()
                .0,
        )
        .unwrap()
    }

    async fn review(pool: &SqlitePool, uid: i64, answer: &str, revision: i64) {
        let _ = cards::check_answer(
            State(pool.clone()),
            AppPath(1),
            AuthUser(uid),
            AppJson(cards::CheckRequest {
                answer: answer.into(),
                sentence_id: 1,
                review_revision: revision,
                content_revision: 0,
            }),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn graded_reviews_store_the_returned_interval_for_correct_and_wrong_answers() {
        let pool = test_pool().await;
        let answer = target(&pool).await;
        for retention in [0.5, 0.9, 0.99] {
            let uid = test_user(&pool).await;
            set_retention(&pool, uid, retention).await;
            for (revision, answer) in [&answer[..], "wrong", &answer[..]].into_iter().enumerate() {
                review(&pool, uid, answer, revision as i64).await;
                let (reviewed, scheduled, stability): (String, f64, f64) = sqlx::query_as(
                    "SELECT reviewed_at, scheduled_days, stability FROM review_history WHERE user_id = ? ORDER BY id DESC LIMIT 1",
                ).bind(uid).fetch_one(&pool).await.unwrap();
                let (last_review, state_stability): (String, f64) = sqlx::query_as(
                    "SELECT last_review, stability FROM cards_states WHERE user_id = ? AND card_id = 1",
                ).bind(uid).fetch_one(&pool).await.unwrap();
                let instant = parse_flexible_datetime(&reviewed).unwrap();
                assert_eq!(instant, parse_flexible_datetime(&last_review).unwrap());
                assert_eq!(stability, state_stability);
                let actual_seconds = (deadline(&pool, uid).await - instant.timestamp()) as f64;
                assert!((0.0..1.0).contains(&(actual_seconds - scheduled * 86400.0)));
                if retention == 0.99 {
                    assert!(scheduled < stability);
                }
                if retention == 0.5 {
                    assert!(scheduled > stability);
                }
            }
        }
    }

    #[tokio::test]
    async fn retention_changes_reschedule_queue_and_stats_without_rewriting_memory_or_history() {
        let pool = test_pool().await;
        let uid = test_user(&pool).await;
        let other = test_user(&pool).await;
        for id in [uid, other] {
            set_retention(&pool, id, 0.9).await;
            review(&pool, id, &target(&pool).await, 0).await;
        }
        // The old 90% deadline is future; the 99% deadline will be overdue.
        sqlx::query(
            "UPDATE cards_states SET last_review = datetime('now','-1 day') WHERE user_id = ?",
        )
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
        set_retention(&pool, uid, 0.9).await;
        let before = export_import::export_data(State(pool.clone()), AuthUser(uid))
            .await
            .unwrap()
            .0;
        let history_before = serde_json::to_value(&before.review_history).unwrap();
        let memory_before: (f64, f64, String, i64) = sqlx::query_as(
            "SELECT stability,difficulty,last_review,review_revision FROM cards_states WHERE user_id=?",
        ).bind(uid).fetch_one(&pool).await.unwrap();
        let other_due = deadline(&pool, other).await;
        let future = next(&pool, uid).await;
        assert!(future["cards"].as_array().unwrap().is_empty());
        assert_eq!(
            future["next_due_at"],
            stats(&pool, uid).await["next_due_at"]
        );
        assert_eq!(stats(&pool, uid).await["due_count"], 0);
        set_retention(&pool, uid, 0.99).await;
        assert_eq!(next(&pool, uid).await["cards"][0]["card_id"], 1);
        assert_eq!(stats(&pool, uid).await["due_count"], 1);
        let memory_after: (f64, f64, String, i64) = sqlx::query_as(
            "SELECT stability,difficulty,last_review,review_revision FROM cards_states WHERE user_id=?",
        ).bind(uid).fetch_one(&pool).await.unwrap();
        assert_eq!(memory_before, memory_after);
        let after = export_import::export_data(State(pool.clone()), AuthUser(uid))
            .await
            .unwrap()
            .0;
        assert_eq!(
            history_before,
            serde_json::to_value(after.review_history).unwrap()
        );
        assert_eq!(other_due, deadline(&pool, other).await);
        set_retention(&pool, uid, 0.5).await;
        assert!(
            next(&pool, uid).await["cards"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(stats(&pool, uid).await["due_count"], 0);
    }

    #[tokio::test]
    async fn trained_decay_and_reset_reschedule_existing_cards() {
        let pool = test_pool().await;
        let uid = test_user(&pool).await;
        set_retention(&pool, uid, 0.99).await;
        review(&pool, uid, &target(&pool).await, 0).await;
        let default_due = deadline(&pool, uid).await;
        let mut parameters = DEFAULT_PARAMETERS;
        parameters[20] = 0.5;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        sqlx::query("INSERT INTO users_fsrs_parameters(user_id,parameters) VALUES (?,?)")
            .bind(uid)
            .bind(serde_json::to_string(&parameters).unwrap())
            .execute(&mut *tx)
            .await
            .unwrap();
        reschedule_user(&mut tx, uid).await.unwrap();
        tx.commit().await.unwrap();
        assert_ne!(deadline(&pool, uid).await, default_due);
        cards::reset_fsrs_parameters(State(pool.clone()), AuthUser(uid))
            .await
            .unwrap();
        assert_eq!(deadline(&pool, uid).await, default_due);
    }

    #[tokio::test]
    async fn backfill_is_restart_safe_uses_current_policy_and_preserves_tombstones() {
        let pool = test_pool().await;
        let uid = test_user(&pool).await;
        set_retention(&pool, uid, 0.99).await;
        review(&pool, uid, &target(&pool).await, 0).await;
        let expected = deadline(&pool, uid).await;
        sqlx::query("UPDATE cards_states SET due_at = NULL WHERE user_id = ?")
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO cards_states(user_id,card_id,review_revision) VALUES (?,2,7)")
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();
        backfill_due_dates(&pool).await.unwrap();
        assert_eq!(deadline(&pool, uid).await, expected);
        // A restart must not reinterpret an already-persisted schedule.
        sqlx::query(
            "UPDATE cards_states SET due_at = due_at + 123 WHERE user_id = ? AND card_id = 1",
        )
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
        backfill_due_dates(&pool).await.unwrap();
        assert_eq!(deadline(&pool, uid).await, expected + 123);
        let tombstone: (Option<i64>, i64) = sqlx::query_as(
            "SELECT due_at,review_revision FROM cards_states WHERE user_id=? AND card_id=2",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(tombstone, (None, 7));
    }

    #[tokio::test]
    async fn export_restore_reconstructs_deadlines_and_rejects_unusable_models_atomically() {
        let pool = test_pool().await;
        let uid = test_user(&pool).await;
        set_retention(&pool, uid, 0.99).await;
        review(&pool, uid, &target(&pool).await, 0).await;
        let expected = deadline(&pool, uid).await;
        let data = export_import::export_data(State(pool.clone()), AuthUser(uid))
            .await
            .unwrap()
            .0;
        let _ = export_import::import_data(
            State(pool.clone()),
            AuthUser(uid),
            AppJson(export_import::ImportDataRequest {
                data,
                overwrite: true,
            }),
        )
        .await
        .unwrap();
        assert_eq!(deadline(&pool, uid).await, expected);
        let mut data = export_import::export_data(State(pool.clone()), AuthUser(uid))
            .await
            .unwrap()
            .0;
        data.settings.fsrs_parameters = Some("[1,2,3]".into());
        assert!(
            export_import::import_data(
                State(pool.clone()),
                AuthUser(uid),
                AppJson(export_import::ImportDataRequest {
                    data,
                    overwrite: true
                })
            )
            .await
            .is_err()
        );
        assert_eq!(deadline(&pool, uid).await, expected);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM review_history WHERE user_id=?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn concurrent_review_and_retention_change_leave_one_consistent_schedule() {
        let pool = test_pool().await;
        let uid = test_user(&pool).await;
        set_retention(&pool, uid, 0.9).await;
        let answer = target(&pool).await;
        tokio::join!(
            review(&pool, uid, &answer, 0),
            set_retention(&pool, uid, 0.99)
        );
        let (last, stability): (String, f64) =
            sqlx::query_as("SELECT last_review,stability FROM cards_states WHERE user_id=?")
                .bind(uid)
                .fetch_one(&pool)
                .await
                .unwrap();
        let scheduler = Scheduler::new(None, 0.99).unwrap();
        assert_eq!(
            deadline(&pool, uid).await,
            scheduler
                .due_at(parse_flexible_datetime(&last).unwrap(), stability)
                .unwrap()
        );
    }
}
