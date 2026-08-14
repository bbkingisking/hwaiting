//! Per-card moderation/curation actions that sit outside the FSRS review
//! flow: content-review comments and the suspend/unsuspend rotation flags.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::info;
use utoipa::ToSchema;

use crate::error::{AppError, AppJson, AppPath};

#[derive(Deserialize, ToSchema)]
pub struct CommentRequest {
    pub body: String,
}

#[derive(Serialize, ToSchema)]
pub struct CommentResponse {
    pub id: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ReviewResponse {
    success: bool,
}

#[derive(Serialize, ToSchema)]
pub struct SuppressedCard {
    card_id: i64,
    word: String,
    trans_word: String,
    sentence: String,
    sentence_translation: String,
    pos: Option<String>,
    grade: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SuppressedCardsResponse {
    cards: Vec<SuppressedCard>,
}

// Record a content-review note against a card - e.g. "tense looks wrong",
// "좀 should be an accepted alternative here". Purely a backlog for admin
// triage; doesn't affect scheduling or what the review UI shows.
#[utoipa::path(
    post,
    path = "/api/cards/{card_id}/comment",
    params(("card_id" = i64, Path, description = "Card ID")),
    request_body = CommentRequest,
    responses(
        (status = 200, description = "Comment recorded", body = CommentResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 404, description = "Card not found", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn comment_on_card(
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    auth: crate::auth::AuthUser,
    AppJson(payload): AppJson<CommentRequest>,
) -> Result<Json<CommentResponse>, AppError> {
    let user_id = auth.0;

    // foreign_keys isn't turned on for this connection (see db.rs), so the
    // FK in the migration is documentation, not enforcement - check by hand
    // so a bad card_id 404s instead of silently inserting an orphaned row.
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM cards WHERE id = ?")
        .bind(card_id)
        .fetch_optional(&pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let id = sqlx::query("INSERT INTO card_comments (card_id, user_id, body) VALUES (?, ?, ?)")
        .bind(card_id)
        .bind(user_id)
        .bind(&payload.body)
        .execute(&pool)
        .await?
        .last_insert_rowid();

    info!(
        "Comment added for card_id: {}, user_id: {}, comment_id: {}",
        card_id, user_id, id
    );

    Ok(Json(CommentResponse { id }))
}

#[utoipa::path(
    put,
    path = "/api/cards/{card_id}/suppress",
    params(("card_id" = i64, Path, description = "Card ID")),
    responses(
        (status = 200, description = "Card suspended from review rotation", body = ReviewResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn suppress_card(
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    auth: crate::auth::AuthUser,
) -> Result<Json<ReviewResponse>, AppError> {
    let user_id = auth.0;
    info!("Suppressing card for user_id: {}, card_id: {}", user_id, card_id);

    // Insert or update user_card_flags to mark as suspended
    sqlx::query(
        r#"
        INSERT INTO user_card_flags (user_id, card_id, suspended)
        VALUES (?, ?, 1)
        ON CONFLICT(user_id, card_id) DO UPDATE SET
            suspended = 1,
            flagged_at = datetime('now')
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .execute(&pool)
    .await?;

    info!("Card suspended successfully");

    Ok(Json(ReviewResponse { success: true }))
}

// List all suspended cards for the user
#[utoipa::path(
    get,
    path = "/api/cards/suppressed",
    responses(
        (status = 200, description = "All suspended cards for the user", body = SuppressedCardsResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn list_suppressed_cards(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<SuppressedCardsResponse>, AppError> {
    let user_id = auth.0;
    info!("Listing suspended cards for user_id: {}", user_id);

    let rows = sqlx::query(
        r#"
        SELECT
            c.id, c.word, pop.slug as pos, g.slug as grade,
            ct.trans_word,
            s.text as sentence,
            st.translation as sentence_translation
        FROM cards c
        INNER JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        INNER JOIN card_translations ct ON c.id = ct.card_id AND ct.language_tag = 'en'
        INNER JOIN sentences s ON c.id = s.card_id
        LEFT JOIN sentence_translations st ON s.id = st.sentence_id
        LEFT JOIN parts_of_speech pop ON pop.id = c.pos_id
        LEFT JOIN grades g ON g.id = c.grade_id
        WHERE ucf.suspended = 1
        ORDER BY c.word ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let cards: Vec<SuppressedCard> = rows
        .iter()
        .map(|row| SuppressedCard {
            card_id: row.get("id"),
            word: row.get("word"),
            trans_word: row.get("trans_word"),
            sentence: row.get("sentence"),
            sentence_translation: row.get("sentence_translation"),
            pos: row.get("pos"),
            grade: row.get("grade"),
        })
        .collect();

    Ok(Json(SuppressedCardsResponse { cards }))
}

#[utoipa::path(
    put,
    path = "/api/cards/{card_id}/unsuppress",
    params(("card_id" = i64, Path, description = "Card ID")),
    responses(
        (status = 200, description = "Card un-suspended", body = ReviewResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn unsuppress_card(
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    auth: crate::auth::AuthUser,
) -> Result<Json<ReviewResponse>, AppError> {
    let user_id = auth.0;
    info!("Unsuspending card for user_id: {}, card_id: {}", user_id, card_id);

    sqlx::query(
        r#"
        UPDATE user_card_flags
        SET suspended = 0
        WHERE user_id = ? AND card_id = ?
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .execute(&pool)
    .await?;

    info!("Card unsuspended successfully");

    Ok(Json(ReviewResponse { success: true }))
}
