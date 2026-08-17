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

    // Group by card_id and build FSRSItem list
    let mut items: Vec<FSRSItem> = Vec::new();
    let mut current_card_id: Option<i64> = None;
    let mut current_reviews: Vec<FSRSReview> = Vec::new();
    let mut last_review_time: Option<chrono::DateTime<Utc>> = None;

    for row in &rows {
        let card_id: i64 = row.get("card_id");
        let rating_str: String = row.get("rating");
        let reviewed_at_str: String = row.get("reviewed_at");

        let rating: u32 = match rating_str.as_str() {
            "again" => 1,
            "hard" => 2,
            "good" => 3,
            "easy" => 4,
            _ => continue,
        };

        let reviewed_at = parse_flexible_datetime(&reviewed_at_str)
            .map_err(|e| AppError::Internal(format!("Invalid date format: {}", e)))?;

        if current_card_id != Some(card_id) {
            // Save previous card's reviews
            if !current_reviews.is_empty() {
                items.push(FSRSItem {
                    reviews: current_reviews,
                });
            }
            current_card_id = Some(card_id);
            current_reviews = Vec::new();
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
        INSERT INTO user_fsrs_parameters (user_id, parameters)
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

    sqlx::query("DELETE FROM user_fsrs_parameters WHERE user_id = ?")
        .bind(user_id)
        .execute(&pool)
        .await?;

    info!("FSRS parameters reset to defaults");

    Ok(StatusCode::NO_CONTENT)
}
