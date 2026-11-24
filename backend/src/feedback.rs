//! Sentence feedback for admin triage.

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::info;
use utoipa::{IntoParams, ToSchema};

use crate::error::{AppError, AppJson, AppPath, AppQuery};

#[derive(Deserialize, ToSchema)]
pub struct FeedbackRequest {
    /// The sentence the feedback is about.
    pub sentence_id: i64,
    /// Stored with leading and trailing whitespace trimmed; must not be empty
    /// after trimming. `maxLength` counts characters as sent.
    #[schema(min_length = 1, max_length = 2000)]
    pub body: String,
}

/// Longest accepted feedback body, in Unicode characters. utoipa's
/// `max_length` needs a literal, so `FeedbackRequest` repeats it.
pub(crate) const FEEDBACK_MAX_CHARS: usize = 2000;

#[derive(Serialize, ToSchema)]
pub struct FeedbackResponse {
    pub id: i64,
}

#[derive(Deserialize, Default, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListFeedbackQuery {
    /// Maximum number of entries. Omit to return all feedback.
    #[param(minimum = 1)]
    pub limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct FeedbackEntry {
    pub id: i64,
    /// Card containing the feedback sentence, for admin editing.
    pub card_id: i64,
    pub sentence_id: i64,
    pub user_id: i64,
    pub body: String,
    /// Submission time (RFC 3339, UTC).
    #[schema(format = DateTime)]
    pub created_at: String,
}

#[derive(Serialize, ToSchema)]
pub struct ListFeedbackResponse {
    pub feedback: Vec<FeedbackEntry>,
}

#[utoipa::path(
    post,
    path = "/api/feedback",
    description = "Submit content feedback about a sentence for admin triage. Any signed-in user may submit; listing requires an admin. Feedback does not change review scheduling or card content.",
    request_body = FeedbackRequest,
    responses(
        (status = 201, description = "Feedback recorded", body = FeedbackResponse),
        (status = 400, description = "Invalid JSON body, or a body that is empty after trimming or longer than 2000 characters", body = crate::error::ErrorResponse),
        (status = 404, description = "Sentence not found", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "feedback"
)]
pub async fn submit_feedback(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
    AppJson(payload): AppJson<FeedbackRequest>,
) -> Result<(StatusCode, Json<FeedbackResponse>), AppError> {
    let user_id = auth.0;
    let sentence_id = payload.sentence_id;
    if payload.body.chars().count() > FEEDBACK_MAX_CHARS {
        return Err(AppError::BadRequest(format!(
            "feedback must be at most {FEEDBACK_MAX_CHARS} characters"
        )));
    }
    let body = payload.body.trim();
    if body.is_empty() {
        return Err(AppError::BadRequest("feedback must not be empty".into()));
    }

    // Return the API's 404 for an unknown sentence. The FK also protects this
    // relationship if another write path forgets the check.
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM sentences WHERE id = ?")
        .bind(sentence_id)
        .fetch_optional(&pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let id = sqlx::query("INSERT INTO feedback (sentence_id, user_id, body) VALUES (?, ?, ?)")
        .bind(sentence_id)
        .bind(user_id)
        .bind(body)
        .execute(&pool)
        .await?
        .last_insert_rowid();

    info!(
        "Feedback submitted for sentence_id: {}, user_id: {}, feedback_id: {}",
        sentence_id, user_id, id
    );

    Ok((StatusCode::CREATED, Json(FeedbackResponse { id })))
}

#[utoipa::path(
    get,
    path = "/api/feedback",
    description = "List everyone's sentence feedback for admin triage, oldest first (submission time, then ID). Omit limit to return all entries. Requires an admin account, unlike submitting feedback.",
    params(ListFeedbackQuery),
    responses(
        (status = 200, description = "Feedback for admin triage", body = ListFeedbackResponse),
        (status = 400, description = "Limit must be a positive integer", body = crate::error::ErrorResponse),
        (status = 403, description = "Admin access required", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "feedback"
)]
pub async fn list_feedback(
    State(pool): State<SqlitePool>,
    _admin: crate::auth::AdminUser,
    AppQuery(params): AppQuery<ListFeedbackQuery>,
) -> Result<Json<ListFeedbackResponse>, AppError> {
    if params.limit.is_some_and(|limit| limit <= 0) {
        return Err(AppError::BadRequest("limit must be positive".into()));
    }
    let feedback = sqlx::query_as::<_, FeedbackEntry>(
        "SELECT f.id, s.card_id, f.sentence_id, f.user_id, f.body, strftime('%Y-%m-%dT%H:%M:%SZ', f.created_at) AS created_at
         FROM feedback f JOIN sentences s ON s.id = f.sentence_id
         ORDER BY f.created_at ASC, f.id ASC LIMIT ?",
    )
    .bind(params.limit.unwrap_or(-1))
    .fetch_all(&pool)
    .await?;
    Ok(Json(ListFeedbackResponse { feedback }))
}

#[utoipa::path(
    delete,
    path = "/api/feedback/{feedback_id}",
    description = "Delete one feedback entry, typically once it has been triaged. Requires an admin account.",
    params(("feedback_id" = i64, Path, description = "Feedback ID")),
    responses(
        (status = 204, description = "Feedback deleted"),
        (status = 400, description = "Non-numeric feedback_id", body = crate::error::ErrorResponse),
        (status = 403, description = "Admin access required", body = crate::error::ErrorResponse),
        (status = 404, description = "No feedback with that ID", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "feedback"
)]
pub async fn delete_feedback(
    State(pool): State<SqlitePool>,
    _admin: crate::auth::AdminUser,
    AppPath(feedback_id): AppPath<i64>,
) -> Result<StatusCode, AppError> {
    let deleted = sqlx::query("DELETE FROM feedback WHERE id = ?")
        .bind(feedback_id)
        .execute(&pool)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(AppError::NotFound);
    }
    info!("Feedback {} deleted", feedback_id);
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
#[path = "tests/feedback.rs"]
mod tests;
