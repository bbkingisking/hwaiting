use axum::{Json, extract::State};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteConnection, SqlitePool};
use utoipa::ToSchema;

use crate::error::{AppError, AppJson};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct AttemptClaims {
    pub version: u8,
    pub user_id: i64,
    pub card_id: i64,
    // Cards can have multiple sentences; this identifies the sentence being graded.
    pub sentence_id: i64,
    // Version of this user's review state; rejects stale submissions after another client reviews.
    pub review_revision: i64,
    // Grading must use the content version returned with the prompt.
    pub content_revision: i64,
}

fn signing_secret() -> String {
    format!("hwaiting:attempt:v1:{}", crate::credentials::jwt_secret())
}

pub(crate) fn issue_attempt_token(
    user_id: i64,
    card_id: i64,
    sentence_id: i64,
    review_revision: i64,
    content_revision: i64,
) -> Result<String, AppError> {
    // No timestamps or randomness: identical prompt state produces identical readable tokens.
    encode(
        &Header::new(Algorithm::HS256),
        &AttemptClaims {
            version: 1,
            user_id,
            card_id,
            sentence_id,
            review_revision,
            content_revision,
        },
        &EncodingKey::from_secret(signing_secret().as_bytes()),
    )
    .map_err(|error| AppError::Internal(format!("Cannot sign attempt token: {error}")))
}

pub(crate) async fn validate_attempt(
    connection: &mut SqliteConnection,
    token: &str,
    user_id: i64,
    card_id: Option<i64>,
) -> Result<AttemptClaims, AppError> {
    let invalid = || AppError::BadRequest("Invalid attempt token".into());
    let mut validation = Validation::new(Algorithm::HS256);
    validation.required_spec_claims.clear();
    validation.validate_exp = false;
    let claims = decode::<AttemptClaims>(
        token,
        &DecodingKey::from_secret(signing_secret().as_bytes()),
        &validation,
    )
    .map_err(|_| invalid())?
    .claims;
    if claims.version != 1
        || claims.user_id != user_id
        || card_id.is_some_and(|id| id != claims.card_id)
    {
        return Err(invalid());
    }
    let row = sqlx::query(
        "SELECT c.content_revision, COALESCE(cs.review_revision, 0) AS review_revision,
                COALESCE(f.suppressed, 0) AS suppressed
         FROM cards c
         LEFT JOIN cards_states cs ON cs.card_id = c.id AND cs.user_id = ?
         LEFT JOIN users_card_flags f ON f.card_id = c.id AND f.user_id = ?
         WHERE c.id = ?",
    )
    .bind(user_id)
    .bind(user_id)
    .bind(claims.card_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AppError::NotFound)?;
    if row.get::<i64, _>("suppressed") != 0 {
        return Err(AppError::Forbidden);
    }
    if claims.review_revision != row.get::<i64, _>("review_revision") {
        return Err(AppError::StaleReview);
    }
    let selected: Option<i64> = sqlx::query_scalar(
        "SELECT s.id FROM sentences s JOIN targets t ON t.sentence_id = s.id
         WHERE s.card_id = ?
         ORDER BY COALESCE((SELECT MAX(rh.id) FROM review_history rh
                            WHERE rh.user_id = ? AND rh.sentence_id = s.id), 0), s.id LIMIT 1",
    )
    .bind(claims.card_id)
    .bind(user_id)
    .fetch_optional(&mut *connection)
    .await?;
    if selected != Some(claims.sentence_id) {
        return Err(AppError::StalePrompt);
    }
    if claims.content_revision != row.get::<i64, _>("content_revision") {
        return Err(AppError::StaleContent);
    }
    Ok(claims)
}

#[derive(Deserialize, ToSchema)]
pub struct ValidateRequest {
    pub attempt_token: String,
}

#[derive(Serialize, ToSchema)]
pub struct ValidateResponse {
    pub valid: bool,
}

#[utoipa::path(
    post,
    path = "/api/cards/validate",
    request_body = ValidateRequest,
    responses(
        (status = 200, description = "Whether the token still matches an available prompt", body = ValidateResponse),
        (status = 400, description = "Malformed request body", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 500, description = "Operational failure", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "cards"
)]
pub async fn validate_card_attempt(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
    AppJson(payload): AppJson<ValidateRequest>,
) -> Result<Json<ValidateResponse>, AppError> {
    // A read-only snapshot is enough here; only grading needs the writer reservation.
    let mut tx = pool.begin().await?;
    let valid = match validate_attempt(&mut tx, &payload.attempt_token, auth.0, None).await {
        Ok(_) => true,
        Err(
            AppError::BadRequest(_)
            | AppError::Forbidden
            | AppError::NotFound
            | AppError::StaleReview
            | AppError::StaleContent
            | AppError::StalePrompt,
        ) => false,
        Err(error) => return Err(error),
    };
    tx.commit().await?;
    Ok(Json(ValidateResponse { valid }))
}
