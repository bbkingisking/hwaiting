//! Per-card moderation/curation actions that sit outside the FSRS review
//! flow: content-review comments and the suppress/unsuppress rotation flags.

use axum::{Json, extract::State};
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
    security(("cookie_auth" = [])),
    tag = "cards"
)]
pub async fn comment_on_card(
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    auth: crate::auth::AuthUser,
    AppJson(payload): AppJson<CommentRequest>,
) -> Result<Json<CommentResponse>, AppError> {
    let user_id = auth.0;

    // Return the API's 404 for an unknown card. The FK also protects this
    // relationship if another write path forgets the check.
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM cards WHERE id = ?")
        .bind(card_id)
        .fetch_optional(&pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let id = sqlx::query("INSERT INTO cards_comments (card_id, user_id, body) VALUES (?, ?, ?)")
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
        (status = 200, description = "Card suppressed from review rotation", body = ReviewResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 404, description = "Card not found", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "cards"
)]
pub async fn suppress_card(
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    auth: crate::auth::AuthUser,
) -> Result<Json<ReviewResponse>, AppError> {
    let user_id = auth.0;
    info!(
        "Suppressing card for user_id: {}, card_id: {}",
        user_id, card_id
    );

    // Select from cards in the same statement so a nonexistent id produces a
    // 404 without ever attempting to insert a dangling flag.
    let result = sqlx::query(
        r#"
        INSERT INTO users_card_flags (user_id, card_id, suppressed)
        SELECT ?, id, 1 FROM cards WHERE id = ?
        ON CONFLICT(user_id, card_id) DO UPDATE SET
            suppressed = 1,
            flagged_at = datetime('now')
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .execute(&pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    info!("Card suppressed successfully");

    Ok(Json(ReviewResponse { success: true }))
}

// List all suppressed cards for the user
#[utoipa::path(
    get,
    path = "/api/cards/suppressed",
    responses(
        (status = 200, description = "All suppressed cards for the user", body = SuppressedCardsResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "cards"
)]
pub async fn list_suppressed_cards(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<SuppressedCardsResponse>, AppError> {
    let user_id = auth.0;
    info!("Listing suppressed cards for user_id: {}", user_id);

    let eng_id = crate::enum_lookup::eng_language_id(&pool).await?;
    let rows = sqlx::query(
        r#"
        SELECT
            c.id, c.word, pop.slug as pos, g.slug as grade,
            ct.trans_word,
            s.text as sentence,
            st.translation as sentence_translation
        FROM cards c
        INNER JOIN users_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        INNER JOIN cards_translations ct ON c.id = ct.card_id AND ct.language_id = ?
        INNER JOIN sentences s ON s.id = (
            SELECT MIN(id) FROM sentences WHERE card_id = c.id
        )
        LEFT JOIN sentences_translations st ON s.id = st.sentence_id AND st.language_id = ?
        LEFT JOIN parts_of_speech pop ON pop.id = c.pos_id
        LEFT JOIN grades g ON g.id = c.grade_id
        WHERE ucf.suppressed = 1
        ORDER BY c.word ASC
        "#,
    )
    .bind(user_id)
    .bind(eng_id)
    .bind(eng_id)
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
        (status = 200, description = "Card unsuppressed", body = ReviewResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "cards"
)]
pub async fn unsuppress_card(
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    auth: crate::auth::AuthUser,
) -> Result<Json<ReviewResponse>, AppError> {
    let user_id = auth.0;
    info!(
        "Unsuppressing card for user_id: {}, card_id: {}",
        user_id, card_id
    );

    sqlx::query(
        r#"
        UPDATE users_card_flags
        SET suppressed = 0
        WHERE user_id = ? AND card_id = ?
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .execute(&pool)
    .await?;

    info!("Card unsuppressed successfully");

    Ok(Json(ReviewResponse { success: true }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auth::AuthUser,
        test_support::{test_pool, test_user},
    };

    #[tokio::test]
    async fn suppressing_missing_card_returns_404_without_a_flag() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        let result = suppress_card(State(pool.clone()), AppPath(999999), AuthUser(user_id)).await;
        assert!(matches!(result, Err(AppError::NotFound)));
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users_card_flags WHERE user_id = ?")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0);

        let response = suppress_card(State(pool.clone()), AppPath(1), AuthUser(user_id))
            .await
            .unwrap();
        assert!(response.0.success);
        let suppressed: bool = sqlx::query_scalar(
            "SELECT suppressed FROM users_card_flags WHERE user_id = ? AND card_id = 1",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(suppressed);
    }
}
