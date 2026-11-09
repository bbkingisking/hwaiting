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
#[path = "../tests/cards/fsrs_admin.rs"]
mod tests;
