//! Per-card moderation/curation actions that sit outside the FSRS review
//! flow: the suppress/unsuppress rotation flags.

use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use tracing::info;
use utoipa::ToSchema;

use crate::error::{AppError, AppPath};

#[derive(Serialize, ToSchema)]
pub struct SuppressedCard {
    card_id: i64,
    word: String,
    trans_word: String,
    sentence: String,
    sentence_translation: String,
    #[schema(required)]
    pos: Option<String>,
    #[schema(required)]
    grade: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SuppressedCardsResponse {
    cards: Vec<SuppressedCard>,
}

#[utoipa::path(
    put,
    path = "/api/cards/{card_id}/suppression",
    description = "Exclude a card from the current user's review rotation. Repeating the request keeps the card suppressed; other users are unaffected.",
    params(("card_id" = i64, Path, description = "Card ID")),
    responses(
        (status = 204, description = "Card suppressed from review rotation"),
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
) -> Result<StatusCode, AppError> {
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

    Ok(StatusCode::NO_CONTENT)
}

// List all suppressed cards for the user
#[utoipa::path(
    get,
    path = "/api/cards/suppressed",
    description = "List the current user's suppressed cards with sentence content and translations in their configured translation language, ordered by Korean word.",
    responses(
        (status = 200, description = "All suppressed cards for the user", body = SuppressedCardsResponse),
        (status = 400, description = "Translation language is not configured or is unknown", body = crate::error::ErrorResponse),
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

    let translation_language =
        crate::translation_language::TranslationLanguage::for_user(&pool, user_id).await?;
    let language_id = translation_language.id(&pool).await?;
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
        INNER JOIN sentences_translations st ON s.id = st.sentence_id AND st.language_id = ?
        LEFT JOIN parts_of_speech pop ON pop.id = c.pos_id
        LEFT JOIN grades g ON g.id = c.grade_id
        WHERE ucf.suppressed = 1
        ORDER BY c.word ASC
        "#,
    )
    .bind(user_id)
    .bind(language_id)
    .bind(language_id)
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
    delete,
    path = "/api/cards/{card_id}/suppression",
    description = "Clear the current user's suppression flag so the card can return to the review rotation when eligible. Repeating the request, or clearing a card that was never suppressed, also succeeds.",
    params(("card_id" = i64, Path, description = "Card ID")),
    responses(
        (status = 204, description = "Card unsuppressed"),
        (status = 404, description = "Card not found", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "cards"
)]
pub async fn unsuppress_card(
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    auth: crate::auth::AuthUser,
) -> Result<StatusCode, AppError> {
    let user_id = auth.0;
    info!(
        "Unsuppressing card for user_id: {}, card_id: {}",
        user_id, card_id
    );

    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cards WHERE id = ?)")
        .bind(card_id)
        .fetch_one(&pool)
        .await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    sqlx::query("UPDATE users_card_flags SET suppressed = 0 WHERE user_id = ? AND card_id = ?")
        .bind(user_id)
        .bind(card_id)
        .execute(&pool)
        .await?;

    info!("Card unsuppressed successfully");

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
#[path = "../tests/cards/moderation.rs"]
mod tests;
