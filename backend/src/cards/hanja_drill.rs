//! A non-graded side activity, entirely separate from the review flow: hand
//! back a random hanja drawn from a card the user has already mastered
//! (`cards_states.state = MASTERED_STATE` - the same definition
//! `cards/stats.rs` uses for the "Mastered" stat) together with its reading
//! and gloss, so the frontend can accept any typed answer as "correct" and
//! reveal both right away. This module only ever runs one read-only SELECT:
//! it never writes to `cards_states` or `review_history`, so there's nothing
//! here for `check_answer`'s grading path to interact with.

use axum::{extract::State, Json};
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use utoipa::ToSchema;

use crate::error::AppError;

use super::MASTERED_STATE;

#[derive(Serialize, ToSchema)]
pub struct HanjaDrill {
    pub hanja: String,
    pub word: String,
    pub trans_word: String,
    pub trans_dfn: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct HanjaDrillResponse {
    /// `None` when the user has no mastered hanja cards yet - a normal empty
    /// state, not an error.
    pub drill: Option<HanjaDrill>,
}

#[utoipa::path(
    get,
    path = "/api/cards/hanja-drill",
    responses(
        (status = 200, description = "A random hanja drawn from the user's mastered cards, or null if none exist yet", body = HanjaDrillResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn get_hanja_drill(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<HanjaDrillResponse>, AppError> {
    let user_id = auth.0;
    let eng_id = crate::enum_lookup::eng_language_id(&pool).await?;

    let row = sqlx::query(
        r#"
        SELECT c.hanja, c.word, ct.trans_word, ct.trans_dfn
        FROM cards c
        INNER JOIN cards_states cs ON cs.card_id = c.id AND cs.user_id = ? AND cs.state = ?
        INNER JOIN cards_translations ct ON ct.card_id = c.id AND ct.language_id = ?
        LEFT JOIN users_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE c.hanja IS NOT NULL AND c.hanja != ''
          AND (ucf.suppressed IS NULL OR ucf.suppressed = 0)
        ORDER BY RANDOM()
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(MASTERED_STATE)
    .bind(eng_id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await?;

    let drill = row.map(|row| HanjaDrill {
        hanja: row.get("hanja"),
        word: row.get("word"),
        trans_word: row.get("trans_word"),
        trans_dfn: row.get("trans_dfn"),
    });

    Ok(Json(HanjaDrillResponse { drill }))
}
