//! User-level FSRS parameter management: `POST /api/cards/fsrs-parameters`
//! fits parameters to the user's own review history via `fsrs::compute_parameters`;
//! `DELETE` on the same path reverts to library defaults. Distinct from
//! [`super::check`], which applies whichever parameters are currently
//! stored to grade a single review.

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use fsrs::{ComputeParametersInput, FSRS, FSRSItem, FSRSReview};
use serde::Serialize;
use sqlx::Row;
use sqlx::SqlitePool;
use tracing::info;
use utoipa::ToSchema;

use crate::error::AppError;

use super::time::parse_flexible_datetime;

#[derive(Serialize, ToSchema)]
pub struct OptimizeFsrsResponse {
    success: bool,
    parameters: Vec<f32>,
    /// Delayed review outcomes supplied to the optimizer, excluding repeated history prefixes.
    review_count: usize,
}

/// One training example per delayed review, containing that card's complete
/// history through the review being predicted. The crate trains against each
/// item's last rating; preceding ratings are inputs, not extra training labels.
/// Same-day reviews remain in the history but are not prediction targets (the
/// same convention as fsrs 5.2.0's Anki converter).
///
/// Parse before sorting: backups can contain both SQLite and RFC3339 timestamps,
/// whose lexical order is not necessarily chronological. Preserve input order
/// for ties; optimize_fsrs supplies database ID order. Return examples in global
/// review-time order because the crate applies recency weights by item position.
fn build_fsrs_items(rows: &[(i64, String, String)]) -> Result<Vec<FSRSItem>, AppError> {
    let mut parsed = Vec::with_capacity(rows.len());
    for (index, (card_id, rating, timestamp)) in rows.iter().enumerate() {
        let rating = match rating.as_str() {
            "again" => 1,
            "hard" => 2,
            "good" => 3,
            "easy" => 4,
            _ => continue,
        };
        let reviewed_at = parse_flexible_datetime(timestamp)
            .map_err(|e| AppError::Internal(format!("Invalid review timestamp: {e}")))?;
        parsed.push((*card_id, reviewed_at, index, rating));
    }
    parsed.sort_by_key(|(card_id, time, index, _)| (*card_id, *time, *index));

    let mut examples = Vec::new();
    let mut current_card_id = None;
    let mut history = Vec::new();
    let mut last_review_time: Option<chrono::DateTime<Utc>> = None;
    for (card_id, reviewed_at, index, rating) in parsed {
        if current_card_id != Some(card_id) {
            history.clear();
            last_review_time = None;
            current_card_id = Some(card_id);
        }
        // Match check_answer's completed-24-hour convention, not a separate
        // calendar-day rule used only by the optimizer.
        let delta_t = last_review_time
            .map(|last| (reviewed_at - last).num_days() as u32)
            .unwrap_or(0);
        history.push(FSRSReview { rating, delta_t });
        if delta_t > 0 {
            examples.push((
                reviewed_at,
                index,
                FSRSItem {
                    reviews: history.clone(),
                },
            ));
        }
        last_review_time = Some(reviewed_at);
    }
    examples.sort_by_key(|(time, index, _)| (*time, *index));
    Ok(examples.into_iter().map(|(_, _, item)| item).collect())
}

// Optimize FSRS parameters from user's review history
#[utoipa::path(
    post,
    path = "/api/cards/fsrs-parameters",
    responses(
        (status = 200, description = "FSRS parameters optimized from full review history", body = OptimizeFsrsResponse),
        (status = 400, description = "No/insufficient review history to optimize from", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "cards"
)]
pub async fn optimize_fsrs(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<OptimizeFsrsResponse>, AppError> {
    let user_id = auth.0;
    info!("Optimizing FSRS parameters for user_id: {}", user_id);

    // Stable tie-break order; build_fsrs_items orders parsed timestamps itself.
    let rows = sqlx::query(
        r#"
        SELECT card_id, rating, reviewed_at
        FROM review_history
        WHERE user_id = ?
        ORDER BY id ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    if rows.is_empty() {
        return Err(AppError::BadRequest("No review history found".to_string()));
    }

    // Copy the rows into plain data before moving CPU-heavy preparation and
    // model fitting off the async request executor.
    let raw_rows: Vec<(i64, String, String)> = rows
        .iter()
        .map(|row| {
            let card_id: i64 = row.get("card_id");
            let rating_str: String = row.get("rating");
            let reviewed_at_str: String = row.get("reviewed_at");
            (card_id, rating_str, reviewed_at_str)
        })
        .collect();

    let (parameters, review_count) = tokio::task::spawn_blocking(move || {
        let items = build_fsrs_items(&raw_rows)?;
        let review_count = items.len();
        if items.is_empty() {
            return Err(AppError::BadRequest(
                "Not enough review history: at least one review must follow an earlier review by a full day.".into(),
            ));
        }
        info!("Built {review_count} FSRS training examples from delayed reviews");
        let fsrs = FSRS::new(None)
            .map_err(|e| AppError::Internal(format!("FSRS init error: {e:?}")))?;
        let parameters = fsrs.compute_parameters(ComputeParametersInput {
            train_set: items,
            progress: None,
            enable_short_term: true,
            num_relearning_steps: None,
        }).map_err(|e| AppError::Internal(format!("FSRS optimization error: {e:?}")))?;
        Ok::<_, AppError>((parameters, review_count))
    })
    .await
    .map_err(|e| AppError::Internal(format!("FSRS optimization task failed: {e}")))??;

    // Commit parameters and their resulting deadlines together.
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    // Store the optimized parameters
    let params_json = serde_json::to_string(&parameters)
        .map_err(|e| AppError::Internal(format!("JSON serialization error: {}", e)))?;

    sqlx::query(
        r#"
        INSERT INTO users_fsrs_parameters (user_id, parameters)
        VALUES (?, ?)
        ON CONFLICT(user_id) DO UPDATE SET parameters = excluded.parameters
        "#,
    )
    .bind(user_id)
    .bind(&params_json)
    .execute(&mut *tx)
    .await?;
    super::scheduling::reschedule_user(&mut tx, user_id).await?;
    tx.commit().await?;

    info!("FSRS parameters optimized from {} reviews", review_count);

    Ok(Json(OptimizeFsrsResponse {
        success: true,
        parameters,
        review_count,
    }))
}

// Reset FSRS parameters to defaults
#[utoipa::path(
    delete,
    path = "/api/cards/fsrs-parameters",
    responses(
        (status = 204, description = "FSRS parameters reset to library defaults"),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "cards"
)]
pub async fn reset_fsrs_parameters(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<StatusCode, AppError> {
    let user_id = auth.0;
    info!("Resetting FSRS parameters for user_id: {}", user_id);

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query("DELETE FROM users_fsrs_parameters WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    super::scheduling::reschedule_user(&mut tx, user_id).await?;
    tx.commit().await?;

    info!("FSRS parameters reset to defaults");

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(card_id: i64, rating: &str, reviewed_at: &str) -> (i64, String, String) {
        (card_id, rating.to_string(), reviewed_at.to_string())
    }

    #[test]
    fn single_card_multiple_reviews_computes_delta_t() {
        let rows = vec![
            row(1, "good", "2026-01-01 00:00:00"),
            row(1, "good", "2026-01-04 00:00:00"),
        ];
        let items = build_fsrs_items(&rows).unwrap();
        assert_eq!(
            items,
            vec![FSRSItem {
                reviews: vec![
                    FSRSReview {
                        rating: 3,
                        delta_t: 0
                    },
                    FSRSReview {
                        rating: 3,
                        delta_t: 3
                    },
                ],
            }]
        );
    }

    #[test]
    fn groups_across_card_boundaries() {
        let rows = vec![
            row(1, "good", "2026-01-01 00:00:00"),
            row(1, "good", "2026-01-03 00:00:00"),
            row(2, "again", "2026-01-01 00:00:00"),
            row(2, "good", "2026-01-05 00:00:00"),
        ];
        let items = build_fsrs_items(&rows).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].reviews.len(), 2);
        assert_eq!(items[1].reviews.len(), 2);
        assert_eq!(items[1].reviews[0].rating, 1);
        assert_eq!(items[1].reviews[1].delta_t, 4);
    }

    #[test]
    fn unknown_rating_is_skipped_without_breaking_the_group() {
        let rows = vec![
            row(1, "good", "2026-01-01 00:00:00"),
            row(1, "bogus", "2026-01-02 00:00:00"),
            row(1, "good", "2026-01-04 00:00:00"),
        ];
        let items = build_fsrs_items(&rows).unwrap();
        assert_eq!(items.len(), 1);
        // The skipped row never touches last_review_time, so delta_t is
        // measured from the last *valid* review (1/1), not the skipped one.
        assert_eq!(
            items[0].reviews,
            vec![
                FSRSReview {
                    rating: 3,
                    delta_t: 0
                },
                FSRSReview {
                    rating: 3,
                    delta_t: 3
                },
            ]
        );
    }

    #[test]
    fn same_day_only_reviews_are_filtered_out() {
        // Every review of card 1 has delta_t == 0 (all on the same day) -
        // FSRS can't train on that, so the item is dropped entirely.
        let rows = vec![
            row(1, "good", "2026-01-01 08:00:00"),
            row(1, "good", "2026-01-01 20:00:00"),
        ];
        let items = build_fsrs_items(&rows).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn first_review_of_a_card_always_has_delta_t_zero() {
        let rows = vec![row(1, "easy", "2026-01-01 00:00:00")];
        let items = build_fsrs_items(&rows).unwrap();
        // A lone review has delta_t 0 and so is filtered by the
        // no-positive-delta_t rule - documents that a single review never
        // contributes a training item on its own.
        assert!(items.is_empty());
    }

    #[test]
    fn unparseable_timestamp_is_an_error() {
        let rows = vec![row(1, "good", "not a date")];
        assert!(build_fsrs_items(&rows).is_err());
    }

    #[test]
    fn every_delayed_review_is_a_target_and_same_day_reviews_remain_history() {
        let items = build_fsrs_items(&[
            row(1, "good", "2026-01-01 08:00:00"),
            row(1, "again", "2026-01-03 08:00:00"),
            row(1, "good", "2026-01-03 09:00:00"),
            row(1, "good", "2026-01-04 10:00:00"),
            row(1, "again", "2026-01-04 11:00:00"),
        ])
        .unwrap();
        assert_eq!(
            items,
            vec![
                FSRSItem {
                    reviews: vec![
                        FSRSReview {
                            rating: 3,
                            delta_t: 0
                        },
                        FSRSReview {
                            rating: 1,
                            delta_t: 2
                        },
                    ]
                },
                FSRSItem {
                    reviews: vec![
                        FSRSReview {
                            rating: 3,
                            delta_t: 0
                        },
                        FSRSReview {
                            rating: 1,
                            delta_t: 2
                        },
                        FSRSReview {
                            rating: 3,
                            delta_t: 0
                        },
                        FSRSReview {
                            rating: 3,
                            delta_t: 1
                        },
                    ]
                },
            ]
        );
    }

    #[test]
    fn examples_are_ordered_by_review_time_not_card_for_recency_weighting() {
        let items = build_fsrs_items(&[
            row(1, "good", "2026-01-01 00:00:00"),
            row(1, "again", "2026-01-10 00:00:00"),
            row(2, "good", "2026-01-02 00:00:00"),
            row(2, "easy", "2026-01-03 00:00:00"),
            row(3, "good", "2026-01-04 00:00:00"),
            row(3, "hard", "2026-01-05 00:00:00"),
        ])
        .unwrap();
        assert_eq!(
            items
                .iter()
                .map(|i| i.reviews.last().unwrap().rating)
                .collect::<Vec<_>>(),
            vec![4, 2, 1]
        );
    }

    #[test]
    fn mixed_timestamp_formats_are_sorted_as_instants_before_computing_elapsed_days() {
        let items = build_fsrs_items(&[
            row(1, "again", "2026-01-03T00:00:00+02:00"), // Jan 2 at 22:00 UTC
            row(1, "easy", "2026-01-02 23:00:00"),
            row(1, "good", "2026-01-01T20:00:00Z"),
            row(1, "hard", "2026-01-04 23:00:00"),
        ])
        .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[1].reviews,
            vec![
                FSRSReview {
                    rating: 3,
                    delta_t: 0
                },
                FSRSReview {
                    rating: 1,
                    delta_t: 1
                },
                FSRSReview {
                    rating: 4,
                    delta_t: 0
                },
                FSRSReview {
                    rating: 2,
                    delta_t: 2
                },
            ]
        );
    }

    #[test]
    fn equal_timestamp_reviews_keep_their_database_order() {
        let items = build_fsrs_items(&[
            row(1, "good", "2026-01-01 00:00:00"),
            row(1, "again", "2026-01-01T00:00:00Z"),
            row(1, "easy", "2026-01-03 00:00:00"),
        ])
        .unwrap();
        assert_eq!(
            items[0]
                .reviews
                .iter()
                .map(|r| r.rating)
                .collect::<Vec<_>>(),
            vec![3, 1, 4]
        );
    }

    #[tokio::test]
    async fn optimization_counts_targets_once_and_reschedules_without_adding_reviews() {
        use crate::{
            auth::AuthUser,
            test_support::{test_pool, test_user},
        };
        let pool = test_pool().await;
        let uid = test_user(&pool).await;
        sqlx::query("INSERT INTO users_settings(user_id,desired_retention) VALUES (?,0.99)")
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO cards_states(user_id,card_id,stability,difficulty,last_review,state,review_revision,due_at) VALUES (?,1,5,3,'2026-01-05 00:00:00','review',3,0)")
            .bind(uid).execute(&pool).await.unwrap();
        for (date, rating) in [
            ("2026-01-01 00:00:00", "good"),
            ("2026-01-03 00:00:00", "again"),
            ("2026-01-05 00:00:00", "good"),
        ] {
            sqlx::query(
                "INSERT INTO review_history(user_id,card_id,rating,reviewed_at) VALUES (?,1,?,?)",
            )
            .bind(uid)
            .bind(rating)
            .bind(date)
            .execute(&pool)
            .await
            .unwrap();
        }
        // A small sample uses the crate's default-parameter fallback. This
        // exercises the real fitting task and its commit/reschedule path.
        let response = optimize_fsrs(State(pool.clone()), AuthUser(uid))
            .await
            .unwrap()
            .0;
        assert_eq!(response.review_count, 2); // Not 1 card, 3 rows, or 2+3 prefix rows.
        let parameters: String =
            sqlx::query_scalar("SELECT parameters FROM users_fsrs_parameters WHERE user_id=?")
                .bind(uid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<f32>>(&parameters).unwrap(),
            response.parameters
        );
        let scheduler = super::super::scheduling::Scheduler::new(Some(&parameters), 0.99).unwrap();
        let (deadline, stability, difficulty, revision): (i64, f64, f64, i64) = sqlx::query_as(
            "SELECT due_at,stability,difficulty,review_revision FROM cards_states WHERE user_id=?",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            deadline,
            scheduler
                .due_at(parse_flexible_datetime("2026-01-05 00:00:00").unwrap(), 5.0)
                .unwrap()
        );
        assert_eq!((stability, difficulty, revision), (5.0, 3.0, 3));
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM review_history WHERE user_id=?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 3);
    }
}
