//! User-level FSRS parameter management: `POST /api/cards/fsrs-parameters`
//! fits parameters to the user's own review history via `fsrs::compute_parameters`;
//! `DELETE` on the same path reverts to library defaults. Distinct from
//! [`super::check`], which applies whichever parameters are currently
//! stored to grade a single review.

use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use fsrs::{ComputeParametersInput, FSRSItem, FSRSReview, FSRS};
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
    review_count: usize,
}

/// Groups flat `(card_id, rating, reviewed_at)` rows - already sorted by
/// `(card_id, reviewed_at ASC)`, same as the query in `optimize_fsrs` - into
/// one `FSRSItem` per card, computing each review's `delta_t` (whole days
/// since that card's previous review; 0 for the first) along the way, then
/// drops any item where every review has `delta_t == 0` (same-day reviews
/// only), which `fsrs::compute_parameters` can't train on. An unrecognized
/// rating string is skipped rather than erroring the whole optimization -
/// only `parse_flexible_datetime` failing on `reviewed_at` is treated as
/// fatal, since a card's delta_t chain would otherwise be silently wrong
/// from that row on. Extracted from `optimize_fsrs` so the grouping/delta_t
/// logic - the past target of the FSRS-optimizer bugs described in this
/// module's git history - can be tested without a database.
fn build_fsrs_items(rows: &[(i64, String, String)]) -> Result<Vec<FSRSItem>, AppError> {
    let mut items: Vec<FSRSItem> = Vec::new();
    let mut current_card_id: Option<i64> = None;
    let mut current_reviews: Vec<FSRSReview> = Vec::new();
    let mut last_review_time: Option<chrono::DateTime<Utc>> = None;

    for (card_id, rating_str, reviewed_at_str) in rows {
        let rating: u32 = match rating_str.as_str() {
            "again" => 1,
            "hard" => 2,
            "good" => 3,
            "easy" => 4,
            _ => continue,
        };

        let reviewed_at = parse_flexible_datetime(reviewed_at_str)
            .map_err(|e| AppError::Internal(format!("Invalid date format: {}", e)))?;

        if current_card_id != Some(*card_id) {
            // Save previous card's reviews
            if !current_reviews.is_empty() {
                items.push(FSRSItem {
                    reviews: std::mem::take(&mut current_reviews),
                });
            }
            current_card_id = Some(*card_id);
            last_review_time = None;
        }

        let delta_t = if let Some(last) = last_review_time {
            (reviewed_at - last).num_days().max(0) as u32
        } else {
            0
        };

        current_reviews.push(FSRSReview { rating, delta_t });
        last_review_time = Some(reviewed_at);
    }

    // Push the last card's reviews
    if !current_reviews.is_empty() {
        items.push(FSRSItem {
            reviews: current_reviews,
        });
    }

    // Filter out items where no review has delta_t > 0 (FSRS requirement)
    items.retain(|item| item.reviews.iter().any(|r| r.delta_t > 0));

    Ok(items)
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
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn optimize_fsrs(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<OptimizeFsrsResponse>, AppError> {
    let user_id = auth.0;
    info!("Optimizing FSRS parameters for user_id: {}", user_id);

    // Fetch all review history for this user, ordered by card and time
    let rows = sqlx::query(
        r#"
        SELECT card_id, rating, reviewed_at
        FROM review_history
        WHERE user_id = ?
        ORDER BY card_id, reviewed_at ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    if rows.is_empty() {
        return Err(AppError::BadRequest("No review history found".to_string()));
    }

    // Group by card_id and build FSRSItem list. rows is ordered by
    // (card_id, reviewed_at ASC), which build_fsrs_items relies on to group
    // correctly - it doesn't re-sort.
    let raw_rows: Vec<(i64, String, String)> = rows
        .iter()
        .map(|row| {
            let card_id: i64 = row.get("card_id");
            let rating_str: String = row.get("rating");
            let reviewed_at_str: String = row.get("reviewed_at");
            (card_id, rating_str, reviewed_at_str)
        })
        .collect();

    let items = build_fsrs_items(&raw_rows)?;

    info!("Built {} FSRS training items from reviews", items.len());

    if items.is_empty() {
        return Err(AppError::BadRequest(
            "Not enough review history. Each card needs at least 2 reviews to optimize.".to_string()
        ));
    }

    let review_count = items.iter().map(|item| item.reviews.len()).sum::<usize>();

    // Run the optimizer
    let fsrs = FSRS::new(None)
        .map_err(|e| AppError::Internal(format!("FSRS init error: {:?}", e)))?;

    let input = ComputeParametersInput {
        train_set: items,
        progress: None,
        enable_short_term: true,
        num_relearning_steps: None,
    };

    let parameters = fsrs.compute_parameters(input)
        .map_err(|e| AppError::Internal(format!("FSRS optimization error: {:?}", e)))?;

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
    .execute(&pool)
    .await?;

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
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn reset_fsrs_parameters(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<StatusCode, AppError> {
    let user_id = auth.0;
    info!("Resetting FSRS parameters for user_id: {}", user_id);

    sqlx::query("DELETE FROM users_fsrs_parameters WHERE user_id = ?")
        .bind(user_id)
        .execute(&pool)
        .await?;

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
                    FSRSReview { rating: 3, delta_t: 0 },
                    FSRSReview { rating: 3, delta_t: 3 },
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
        assert_eq!(items[0].reviews, vec![
            FSRSReview { rating: 3, delta_t: 0 },
            FSRSReview { rating: 3, delta_t: 3 },
        ]);
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
}
