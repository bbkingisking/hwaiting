use axum::{
    extract::State,
    http::{header, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::info;
use utoipa::ToSchema;

use crate::error::{AppError, AppJson, AppPath};
use crate::auth::AuthUser;

#[derive(Deserialize, ToSchema)]
pub struct CreateCustomCardRequest {
    pub word: String,
    pub definition: Option<String>,
    pub trans_word: String,
    pub trans_dfn: Option<String>,
    pub sentence: String,
    pub target: String,
    pub sentence_translation: String,
    #[serde(flatten)]
    pub inflection_hint: crate::inflection_hints::InflectionHintWrite,
    pub pos: Option<String>,
    pub grade: Option<String>,
    pub origin_type: Option<String>,
    pub hanja: Option<String>,
    pub alternatives: Option<Vec<String>>,
}

#[derive(Serialize, ToSchema)]
pub struct CreateCustomCardResponse {
    pub id: i64,
    pub success: bool,
}

#[derive(Serialize, ToSchema)]
pub struct CustomCard {
    pub id: i64,
    pub word: String,
    pub definition: Option<String>,
    pub trans_word: String,
    pub trans_dfn: Option<String>,
    pub sentence: String,
    pub target: String,
    /// Was missing here until this field was added: `CreateCustomCardRequest`,
    /// `UpdateCustomCardRequest`, and export/import's `SentenceExport` all
    /// accept/carry a card's alternative accepted answers, but this read
    /// shape - what `GET /api/custom-cards` and `GET /api/custom-cards/{id}`
    /// actually return - silently dropped them, so there was no way to see
    /// (or build an edit form around) alternatives you'd already set outside
    /// of a full data export. Same class of bug as `cards::Card` drifting
    /// from `CardPrompt`/`CardReveal` - independently hand-declared shapes
    /// of "the same card" agreeing on every field but one.
    pub alternatives: Vec<String>,
    pub sentence_translation: String,
    #[serde(flatten)]
    pub inflection_hint: crate::inflection_hints::InflectionHint,
    pub pos: Option<String>,
    pub grade: Option<String>,
    pub origin_type: Option<String>,
    pub hanja: Option<String>,
    pub created_at: String,
}

#[derive(Serialize, ToSchema)]
pub struct ListCustomCardsResponse {
    pub cards: Vec<CustomCard>,
}

// Shared by list_custom_cards and get_custom_card, which differ only in
// their WHERE clause (all cards owned by the user vs. one by id) - keeping
// the join/column list in one place means the two can't drift into
// returning different shapes for what's supposed to be the same read model.
const CUSTOM_CARD_SELECT: &str = r#"
    SELECT
        c.id,
        c.word,
        c.definition,
        pop.slug as pos,
        g.slug as grade,
        ot.slug as origin_type,
        c.hanja,
        ct.trans_word,
        ct.trans_dfn,
        s.id as sentence_id,
        s.text as sentence,
        tg.form as target,
        st.translation as sentence_translation,
        sl.slug as speech_level,
        tn.slug as tense,
        tg.is_honorific,
        tg.is_humble,
        datetime(ccm.created_at) as created_at
    FROM cards c
    INNER JOIN custom_card_metadata ccm ON c.id = ccm.card_id
    INNER JOIN card_translations ct ON c.id = ct.card_id AND ct.language_tag = 'en'
    INNER JOIN sentences s ON c.id = s.card_id
    INNER JOIN targets tg ON tg.sentence_id = s.id
    LEFT JOIN sentence_translations st ON s.id = st.sentence_id
    LEFT JOIN parts_of_speech pop ON pop.id = c.pos_id
    LEFT JOIN grades g ON g.id = c.grade_id
    LEFT JOIN origin_types ot ON ot.id = c.origin_type_id
    LEFT JOIN speech_levels sl ON sl.id = tg.speech_level_id
    LEFT JOIN tenses tn ON tn.id = tg.tense_id
"#;

// Alternatives live in a join-table keyed by sentence_id rather than a
// column CUSTOM_CARD_SELECT's row can carry directly, so this fetches them
// in a second query per row - same N+1 pattern admin::search_cards already
// uses for the same reason.
async fn custom_card_from_row(
    pool: &SqlitePool,
    row: &sqlx::sqlite::SqliteRow,
) -> Result<CustomCard, AppError> {
    let sentence_id: i64 = row.get("sentence_id");
    let alternatives: Vec<String> = sqlx::query_scalar(
        "SELECT alt_target FROM target_alternatives WHERE sentence_id = ?",
    )
    .bind(sentence_id)
    .fetch_all(pool)
    .await?;

    Ok(CustomCard {
        id: row.get("id"),
        word: row.get("word"),
        definition: row.get("definition"),
        trans_word: row.get("trans_word"),
        trans_dfn: row.get("trans_dfn"),
        sentence: row.get("sentence"),
        target: row.get("target"),
        alternatives,
        sentence_translation: row.get("sentence_translation"),
        inflection_hint: crate::inflection_hints::InflectionHint::from_row(row),
        pos: row.get("pos"),
        grade: row.get("grade"),
        origin_type: row.get("origin_type"),
        hanja: row.get("hanja"),
        created_at: row.get("created_at"),
    })
}

// Create a new custom card
#[utoipa::path(
    post,
    path = "/api/custom-cards",
    request_body = CreateCustomCardRequest,
    responses(
        (status = 201, description = "Custom card created", body = CreateCustomCardResponse),
        (status = 400, description = "Empty required field or target not in sentence", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "custom-cards"
)]
pub async fn create_custom_card(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    AppJson(payload): AppJson<CreateCustomCardRequest>,
) -> Result<(StatusCode, [(header::HeaderName, String); 1], Json<CreateCustomCardResponse>), AppError> {
    let user_id = auth.0;
    info!("Creating custom card for user_id: {}", user_id);

    // Validate required fields
    if payload.word.trim().is_empty() {
        return Err(AppError::BadRequest("Word cannot be empty".to_string()));
    }
    if payload.trans_word.trim().is_empty() {
        return Err(AppError::BadRequest("Translation cannot be empty".to_string()));
    }
    if payload.sentence.trim().is_empty() {
        return Err(AppError::BadRequest("Sentence cannot be empty".to_string()));
    }
    if payload.target.trim().is_empty() {
        return Err(AppError::BadRequest("Target cannot be empty".to_string()));
    }
    if payload.sentence_translation.trim().is_empty() {
        return Err(AppError::BadRequest("Sentence translation cannot be empty".to_string()));
    }

    // Validate that target appears in sentence
    if !payload.sentence.contains(&payload.target) {
        return Err(AppError::BadRequest(
            "Target word must appear in the sentence".to_string(),
        ));
    }

    let mut tx = pool.begin().await?;

    // Resolve enum slugs -> lookup table ids
    let pos_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "parts_of_speech", payload.pos.clone().map(Some)).await?.flatten();
    let grade_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "grades", payload.grade.clone().map(Some)).await?.flatten();
    let origin_type_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "origin_types", payload.origin_type.clone().map(Some)).await?.flatten();

    // Insert into cards table
    let card_result = sqlx::query(
        r#"
        INSERT INTO cards (word, definition, pos_id, grade_id, origin_type_id, hanja, created_at)
        VALUES (?, ?, ?, ?, ?, ?, datetime('now'))
        "#
    )
    .bind(&payload.word)
    .bind(&payload.definition)
    .bind(pos_id)
    .bind(grade_id)
    .bind(origin_type_id)
    .bind(&payload.hanja)
    .execute(&mut *tx)
    .await?;

    let card_id = card_result.last_insert_rowid();

    // Insert into custom_card_metadata
    sqlx::query(
        r#"
        INSERT INTO custom_card_metadata (card_id, user_id, created_at)
        VALUES (?, ?, datetime('now'))
        "#
    )
    .bind(card_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    // Insert into card_translations
    sqlx::query(
        r#"
        INSERT INTO card_translations (card_id, language_tag, trans_word, trans_dfn)
        VALUES (?, 'en', ?, ?)
        "#
    )
    .bind(card_id)
    .bind(&payload.trans_word)
    .bind(&payload.trans_dfn)
    .execute(&mut *tx)
    .await?;

    // Insert into sentences
    let sentence_result = sqlx::query(
        r#"
        INSERT INTO sentences (card_id, text, created_at)
        VALUES (?, ?, datetime('now'))
        "#
    )
    .bind(card_id)
    .bind(&payload.sentence)
    .execute(&mut *tx)
    .await?;

    let sentence_id = sentence_result.last_insert_rowid();

    // Insert into sentence_translations
    sqlx::query(
        r#"
        INSERT INTO sentence_translations (sentence_id, translation)
        VALUES (?, ?)
        "#
    )
    .bind(sentence_id)
    .bind(&payload.sentence_translation)
    .execute(&mut *tx)
    .await?;

    // Insert into targets - unconditional (unlike the old
    // sentence_inflection_hints, which only got a row when at least one
    // hint field was set), since `form` is required on every sentence.
    let speech_level_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "speech_levels", payload.inflection_hint.speech_level.clone().map(Some)).await?.flatten();
    let tense_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "tenses", payload.inflection_hint.tense.clone().map(Some)).await?.flatten();
    sqlx::query(
        "INSERT INTO targets (sentence_id, form, speech_level_id, tense_id, is_honorific, is_humble) VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(sentence_id)
    .bind(&payload.target)
    .bind(speech_level_id)
    .bind(tense_id)
    .bind(payload.inflection_hint.is_honorific.unwrap_or(false))
    .bind(payload.inflection_hint.is_humble.unwrap_or(false))
    .execute(&mut *tx)
    .await?;

    // Insert alternative targets
    if let Some(ref alts) = payload.alternatives {
        for alt in alts {
            let trimmed = alt.trim();
            if !trimmed.is_empty() {
                sqlx::query(
                    "INSERT INTO target_alternatives (sentence_id, alt_target) VALUES (?, ?)"
                )
                .bind(sentence_id)
                .bind(trimmed)
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    tx.commit().await?;

    info!("Custom card created successfully with id: {}", card_id);

    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, format!("/api/custom-cards/{}", card_id))],
        Json(CreateCustomCardResponse {
            id: card_id,
            success: true,
        }),
    ))
}

// List all custom cards for the authenticated user
#[utoipa::path(
    get,
    path = "/api/custom-cards",
    responses(
        (status = 200, description = "All custom cards owned by the caller", body = ListCustomCardsResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "custom-cards"
)]
pub async fn list_custom_cards(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<ListCustomCardsResponse>, AppError> {
    let user_id = auth.0;
    info!("Listing custom cards for user_id: {}", user_id);

    let rows = sqlx::query(&format!(
        "{CUSTOM_CARD_SELECT} WHERE ccm.user_id = ? ORDER BY ccm.created_at DESC"
    ))
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let mut cards = Vec::with_capacity(rows.len());
    for row in &rows {
        cards.push(custom_card_from_row(&pool, row).await?);
    }

    Ok(Json(ListCustomCardsResponse { cards }))
}

// Delete a custom card
#[utoipa::path(
    delete,
    path = "/api/custom-cards/{card_id}",
    params(("card_id" = i64, Path, description = "Custom card ID")),
    responses(
        (status = 204, description = "Custom card deleted"),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 404, description = "Not found, or not owned by caller", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "custom-cards"
)]
pub async fn delete_custom_card(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    AppPath(card_id): AppPath<i64>,
) -> Result<StatusCode, AppError> {
    let user_id = auth.0;
    info!("Deleting custom card {} for user_id: {}", card_id, user_id);

    // Ensure the card belongs to the user by deleting from custom_card_metadata
    // The CASCADE will handle cleanup of related tables
    let result = sqlx::query(
        "DELETE FROM custom_card_metadata WHERE card_id = ? AND user_id = ?"
    )
    .bind(card_id)
    .bind(user_id)
    .execute(&pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    // Manually delete from cards table since custom_card_metadata -> cards is ON DELETE CASCADE
    // but cards is the parent, so we need to delete it explicitly
    sqlx::query("DELETE FROM cards WHERE id = ?")
        .bind(card_id)
        .execute(&pool)
        .await?;

    info!("Custom card deleted successfully");

    Ok(StatusCode::NO_CONTENT)
}

// Get a single custom card by ID
#[utoipa::path(
    get,
    path = "/api/custom-cards/{card_id}",
    params(("card_id" = i64, Path, description = "Custom card ID")),
    responses(
        (status = 200, description = "Single custom card", body = CustomCard),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 404, description = "Not found, or not owned by caller", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "custom-cards"
)]
pub async fn get_custom_card(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    AppPath(card_id): AppPath<i64>,
) -> Result<Json<CustomCard>, AppError> {
    let user_id = auth.0;
    info!("Getting custom card {} for user_id: {}", card_id, user_id);

    let row = sqlx::query(&format!("{CUSTOM_CARD_SELECT} WHERE c.id = ? AND ccm.user_id = ?"))
        .bind(card_id)
        .bind(user_id)
        .fetch_optional(&pool)
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(Json(custom_card_from_row(&pool, &row).await?))
}

// Update a custom card
#[derive(Deserialize, ToSchema)]
pub struct UpdateCustomCardRequest {
    pub word: Option<String>,
    pub definition: Option<String>,
    pub trans_word: Option<String>,
    pub trans_dfn: Option<String>,
    pub sentence: Option<String>,
    pub target: Option<String>,
    pub sentence_translation: Option<String>,
    #[serde(flatten)]
    pub inflection_hint: crate::inflection_hints::InflectionHintWrite,
    pub pos: Option<String>,
    pub grade: Option<String>,
    pub origin_type: Option<String>,
    pub hanja: Option<String>,
    pub alternatives: Option<Vec<String>>,
}

#[derive(Serialize, ToSchema)]
pub struct UpdateCustomCardResponse {
    pub success: bool,
}

#[utoipa::path(
    patch,
    path = "/api/custom-cards/{card_id}",
    params(("card_id" = i64, Path, description = "Custom card ID")),
    request_body = UpdateCustomCardRequest,
    responses(
        (status = 200, description = "Custom card updated (partial update)", body = UpdateCustomCardResponse),
        (status = 400, description = "Empty field or target not in sentence", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 404, description = "Not found, or not owned by caller", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "custom-cards"
)]
pub async fn update_custom_card(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    AppPath(card_id): AppPath<i64>,
    AppJson(payload): AppJson<UpdateCustomCardRequest>,
) -> Result<Json<UpdateCustomCardResponse>, AppError> {
    let user_id = auth.0;
    info!("Updating custom card {} for user_id: {}", card_id, user_id);

    // Verify the card exists and belongs to the user
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT card_id FROM custom_card_metadata WHERE card_id = ? AND user_id = ?"
    )
    .bind(card_id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await?;

    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let mut tx = pool.begin().await?;

    // Update cards table
    if let Some(word) = &payload.word {
        if word.trim().is_empty() {
            return Err(AppError::BadRequest("Word cannot be empty".to_string()));
        }
        sqlx::query("UPDATE cards SET word = ? WHERE id = ?")
            .bind(word)
            .bind(card_id)
            .execute(&mut *tx)
            .await?;
    }

    if payload.definition.is_some() {
        sqlx::query("UPDATE cards SET definition = ? WHERE id = ?")
            .bind(&payload.definition)
            .bind(card_id)
            .execute(&mut *tx)
            .await?;
    }

    if payload.pos.is_some() {
        let pos_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "parts_of_speech", payload.pos.clone().map(Some)).await?.flatten();
        sqlx::query("UPDATE cards SET pos_id = ? WHERE id = ?")
            .bind(pos_id)
            .bind(card_id)
            .execute(&mut *tx)
            .await?;
    }

    if payload.grade.is_some() {
        let grade_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "grades", payload.grade.clone().map(Some)).await?.flatten();
        sqlx::query("UPDATE cards SET grade_id = ? WHERE id = ?")
            .bind(grade_id)
            .bind(card_id)
            .execute(&mut *tx)
            .await?;
    }

    if payload.origin_type.is_some() {
        let origin_type_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "origin_types", payload.origin_type.clone().map(Some)).await?.flatten();
        sqlx::query("UPDATE cards SET origin_type_id = ? WHERE id = ?")
            .bind(origin_type_id)
            .bind(card_id)
            .execute(&mut *tx)
            .await?;
    }

    if payload.hanja.is_some() {
        sqlx::query("UPDATE cards SET hanja = ? WHERE id = ?")
            .bind(&payload.hanja)
            .bind(card_id)
            .execute(&mut *tx)
            .await?;
    }

    // Update card_translations
    if let Some(trans_word) = &payload.trans_word {
        if trans_word.trim().is_empty() {
            return Err(AppError::BadRequest("Translation cannot be empty".to_string()));
        }
        sqlx::query("UPDATE card_translations SET trans_word = ? WHERE card_id = ? AND language_tag = 'en'")
            .bind(trans_word)
            .bind(card_id)
            .execute(&mut *tx)
            .await?;
    }

    if payload.trans_dfn.is_some() {
        sqlx::query("UPDATE card_translations SET trans_dfn = ? WHERE card_id = ? AND language_tag = 'en'")
            .bind(&payload.trans_dfn)
            .bind(card_id)
            .execute(&mut *tx)
            .await?;
    }

    // Resolve this card's sentence once - shared by the sentence-text
    // update, the target/hint update, and the alternatives update below.
    let sentence_id: i64 = sqlx::query_scalar(
        "SELECT id FROM sentences WHERE card_id = ? LIMIT 1"
    )
    .bind(card_id)
    .fetch_one(&mut *tx)
    .await?;

    // Validate sentence/target together before writing either. text and
    // target live on separate tables (sentences.text / targets.form - see
    // migration 20240101000026), so whichever side isn't being edited has
    // to be read from wherever it actually lives.
    if payload.sentence.is_some() || payload.target.is_some() {
        let effective_sentence = match &payload.sentence {
            Some(v) => v.clone(),
            None => sqlx::query_scalar("SELECT text FROM sentences WHERE id = ?")
                .bind(sentence_id)
                .fetch_one(&mut *tx)
                .await?,
        };
        let effective_target = match &payload.target {
            Some(v) => v.clone(),
            None => sqlx::query_scalar("SELECT form FROM targets WHERE sentence_id = ?")
                .bind(sentence_id)
                .fetch_one(&mut *tx)
                .await?,
        };

        if effective_sentence.trim().is_empty() {
            return Err(AppError::BadRequest("Sentence cannot be empty".to_string()));
        }
        if effective_target.trim().is_empty() {
            return Err(AppError::BadRequest("Target cannot be empty".to_string()));
        }
        if !effective_sentence.contains(&effective_target) {
            return Err(AppError::BadRequest(
                "Target word must appear in the sentence".to_string(),
            ));
        }
    }

    if let Some(sentence) = &payload.sentence {
        sqlx::query("UPDATE sentences SET text = ? WHERE id = ?")
            .bind(sentence)
            .bind(sentence_id)
            .execute(&mut *tx)
            .await?;
    }

    // Update sentence_translations
    if let Some(sentence_translation) = &payload.sentence_translation {
        if sentence_translation.trim().is_empty() {
            return Err(AppError::BadRequest("Sentence translation cannot be empty".to_string()));
        }
        sqlx::query("UPDATE sentence_translations SET translation = ? WHERE sentence_id = ?")
            .bind(sentence_translation)
            .bind(sentence_id)
            .execute(&mut *tx)
            .await?;
    }

    // Update targets (form / speech_level / tense / is_honorific /
    // is_humble). No exists-check/insert branch needed here unlike the old
    // sentence_inflection_hints: a `targets` row is created unconditionally
    // alongside every sentence now (its `form` is NOT NULL), never left
    // absent the way hint rows used to be.
    if let Some(target) = &payload.target {
        sqlx::query("UPDATE targets SET form = ? WHERE sentence_id = ?")
            .bind(target)
            .bind(sentence_id)
            .execute(&mut *tx)
            .await?;
    }

    let speech_level_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "speech_levels", payload.inflection_hint.speech_level.clone().map(Some)).await?.flatten();
    let tense_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "tenses", payload.inflection_hint.tense.clone().map(Some)).await?.flatten();

    if payload.inflection_hint.speech_level.is_some() {
        sqlx::query("UPDATE targets SET speech_level_id = ? WHERE sentence_id = ?")
            .bind(speech_level_id)
            .bind(sentence_id)
            .execute(&mut *tx)
            .await?;
    }

    if payload.inflection_hint.tense.is_some() {
        sqlx::query("UPDATE targets SET tense_id = ? WHERE sentence_id = ?")
            .bind(tense_id)
            .bind(sentence_id)
            .execute(&mut *tx)
            .await?;
    }

    if let Some(v) = payload.inflection_hint.is_honorific {
        sqlx::query("UPDATE targets SET is_honorific = ? WHERE sentence_id = ?")
            .bind(v)
            .bind(sentence_id)
            .execute(&mut *tx)
            .await?;
    }

    if let Some(v) = payload.inflection_hint.is_humble {
        sqlx::query("UPDATE targets SET is_humble = ? WHERE sentence_id = ?")
            .bind(v)
            .bind(sentence_id)
            .execute(&mut *tx)
            .await?;
    }

    // Update alternative targets
    if let Some(ref alts) = payload.alternatives {
        // Delete existing
        sqlx::query("DELETE FROM target_alternatives WHERE sentence_id = ?")
            .bind(sentence_id)
            .execute(&mut *tx)
            .await?;

        // Insert new
        for alt in alts {
            let trimmed = alt.trim();
            if !trimmed.is_empty() {
                sqlx::query(
                    "INSERT INTO target_alternatives (sentence_id, alt_target) VALUES (?, ?)"
                )
                .bind(sentence_id)
                .bind(trimmed)
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    tx.commit().await?;

    info!("Custom card updated successfully");

    Ok(Json(UpdateCustomCardResponse { success: true }))
}