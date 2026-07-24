use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::info;

use crate::error::AppError;
use crate::auth::AuthUser;

#[derive(Deserialize)]
pub struct CreateCustomCardRequest {
    pub word: String,
    pub definition: Option<String>,
    pub trans_word: String,
    pub trans_dfn: Option<String>,
    pub sentence: String,
    pub target: String,
    pub sentence_translation: String,
    pub speech_level: Option<String>,
    pub tense: Option<String>,
    pub pos: Option<String>,
    pub grade: Option<String>,
    pub origin_type: Option<String>,
    pub hanja: Option<String>,
    pub hanja_eum: Option<String>,
    pub alternatives: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct CreateCustomCardResponse {
    pub id: i64,
    pub success: bool,
}

#[derive(Serialize)]
pub struct CustomCard {
    pub id: i64,
    pub word: String,
    pub definition: Option<String>,
    pub trans_word: String,
    pub trans_dfn: Option<String>,
    pub sentence: String,
    pub target: String,
    pub sentence_translation: String,
    pub speech_level: Option<String>,
    pub tense: Option<String>,
    pub pos: Option<String>,
    pub grade: Option<String>,
    pub origin_type: Option<String>,
    pub hanja: Option<String>,
    pub hanja_eum: Option<String>,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ListCustomCardsResponse {
    pub cards: Vec<CustomCard>,
}

#[derive(Serialize)]
pub struct DeleteCustomCardResponse {
    pub success: bool,
}

// Create a new custom card
pub async fn create_custom_card(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Json(payload): Json<CreateCustomCardRequest>,
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
        INSERT INTO cards (word, definition, pos_id, grade_id, origin_type_id, hanja, hanja_eum, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, datetime('now'))
        "#
    )
    .bind(&payload.word)
    .bind(&payload.definition)
    .bind(pos_id)
    .bind(grade_id)
    .bind(origin_type_id)
    .bind(&payload.hanja)
    .bind(&payload.hanja_eum)
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
        INSERT INTO sentences (card_id, text, target, created_at)
        VALUES (?, ?, ?, datetime('now'))
        "#
    )
    .bind(card_id)
    .bind(&payload.sentence)
    .bind(&payload.target)
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

    // Insert into sentence_inflection_hints if either speech_level or tense is provided
    let speech_level_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "speech_levels", payload.speech_level.clone().map(Some)).await?.flatten();
    let tense_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "tenses", payload.tense.clone().map(Some)).await?.flatten();
    if speech_level_id.is_some() || tense_id.is_some() {
        sqlx::query(
            "INSERT INTO sentence_inflection_hints (sentence_id, speech_level_id, tense_id) VALUES (?, ?, ?)"
        )
        .bind(sentence_id)
        .bind(speech_level_id)
        .bind(tense_id)
        .execute(&mut *tx)
        .await?;
    }

    // Insert alternative targets
    if let Some(ref alts) = payload.alternatives {
        for alt in alts {
            let trimmed = alt.trim();
            if !trimmed.is_empty() {
                sqlx::query(
                    "INSERT INTO sentence_alternative_targets (sentence_id, alt_target) VALUES (?, ?)"
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
pub async fn list_custom_cards(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<ListCustomCardsResponse>, AppError> {
    let user_id = auth.0;
    info!("Listing custom cards for user_id: {}", user_id);

    let rows = sqlx::query(
        r#"
        SELECT
            c.id,
            c.word,
            c.definition,
            pop.slug as pos,
            g.slug as grade,
            ot.slug as origin_type,
            c.hanja,
            c.hanja_eum,
            ct.trans_word,
            ct.trans_dfn,
            s.text as sentence,
            s.target,
            st.translation as sentence_translation,
            sl.slug as speech_level,
            tn.slug as tense,
            datetime(ccm.created_at) as created_at
        FROM cards c
        INNER JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        INNER JOIN card_translations ct ON c.id = ct.card_id AND ct.language_tag = 'en'
        INNER JOIN sentences s ON c.id = s.card_id
        LEFT JOIN sentence_translations st ON s.id = st.sentence_id
        LEFT JOIN sentence_inflection_hints sih ON s.id = sih.sentence_id
        LEFT JOIN parts_of_speech pop ON pop.id = c.pos_id
        LEFT JOIN grades g ON g.id = c.grade_id
        LEFT JOIN origin_types ot ON ot.id = c.origin_type_id
        LEFT JOIN speech_levels sl ON sl.id = sih.speech_level_id
        LEFT JOIN tenses tn ON tn.id = sih.tense_id
        WHERE ccm.user_id = ?
        ORDER BY ccm.created_at DESC
        "#
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let cards = rows.into_iter().map(|row| {
        CustomCard {
            id: row.get("id"),
            word: row.get("word"),
            definition: row.get("definition"),
            trans_word: row.get("trans_word"),
            trans_dfn: row.get("trans_dfn"),
            sentence: row.get("sentence"),
            target: row.get("target"),
            sentence_translation: row.get("sentence_translation"),
            speech_level: row.get("speech_level"),
            tense: row.get("tense"),
            pos: row.get("pos"),
            grade: row.get("grade"),
            origin_type: row.get("origin_type"),
            hanja: row.get("hanja"),
            hanja_eum: row.get("hanja_eum"),
            created_at: row.get("created_at"),
        }
    }).collect();

    Ok(Json(ListCustomCardsResponse { cards }))
}

// Delete a custom card
pub async fn delete_custom_card(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Path(card_id): Path<i64>,
) -> Result<Json<DeleteCustomCardResponse>, AppError> {
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

    Ok(Json(DeleteCustomCardResponse { success: true }))
}

// Get a single custom card by ID
pub async fn get_custom_card(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Path(card_id): Path<i64>,
) -> Result<Json<CustomCard>, AppError> {
    let user_id = auth.0;
    info!("Getting custom card {} for user_id: {}", card_id, user_id);

    let row = sqlx::query(
        r#"
        SELECT
            c.id,
            c.word,
            c.definition,
            pop.slug as pos,
            g.slug as grade,
            ot.slug as origin_type,
            c.hanja,
            c.hanja_eum,
            ct.trans_word,
            ct.trans_dfn,
            s.text as sentence,
            s.target,
            st.translation as sentence_translation,
            sl.slug as speech_level,
            tn.slug as tense,
            datetime(ccm.created_at) as created_at
        FROM cards c
        INNER JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        INNER JOIN card_translations ct ON c.id = ct.card_id AND ct.language_tag = 'en'
        INNER JOIN sentences s ON c.id = s.card_id
        LEFT JOIN sentence_translations st ON s.id = st.sentence_id
        LEFT JOIN sentence_inflection_hints sih ON s.id = sih.sentence_id
        LEFT JOIN parts_of_speech pop ON pop.id = c.pos_id
        LEFT JOIN grades g ON g.id = c.grade_id
        LEFT JOIN origin_types ot ON ot.id = c.origin_type_id
        LEFT JOIN speech_levels sl ON sl.id = sih.speech_level_id
        LEFT JOIN tenses tn ON tn.id = sih.tense_id
        WHERE c.id = ? AND ccm.user_id = ?
        "#
    )
    .bind(card_id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await?;

    let row = row.ok_or(AppError::NotFound)?;

    Ok(Json(CustomCard {
        id: row.get("id"),
        word: row.get("word"),
        definition: row.get("definition"),
        trans_word: row.get("trans_word"),
        trans_dfn: row.get("trans_dfn"),
        sentence: row.get("sentence"),
        target: row.get("target"),
        sentence_translation: row.get("sentence_translation"),
        speech_level: row.get("speech_level"),
        tense: row.get("tense"),
        pos: row.get("pos"),
        grade: row.get("grade"),
        origin_type: row.get("origin_type"),
        hanja: row.get("hanja"),
        hanja_eum: row.get("hanja_eum"),
        created_at: row.get("created_at"),
    }))
}

// Update a custom card
#[derive(Deserialize)]
pub struct UpdateCustomCardRequest {
    pub word: Option<String>,
    pub definition: Option<String>,
    pub trans_word: Option<String>,
    pub trans_dfn: Option<String>,
    pub sentence: Option<String>,
    pub target: Option<String>,
    pub sentence_translation: Option<String>,
    pub speech_level: Option<String>,
    pub tense: Option<String>,
    pub pos: Option<String>,
    pub grade: Option<String>,
    pub origin_type: Option<String>,
    pub hanja: Option<String>,
    pub hanja_eum: Option<String>,
    pub alternatives: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct UpdateCustomCardResponse {
    pub success: bool,
}

pub async fn update_custom_card(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Path(card_id): Path<i64>,
    Json(payload): Json<UpdateCustomCardRequest>,
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

    if payload.hanja_eum.is_some() {
        sqlx::query("UPDATE cards SET hanja_eum = ? WHERE id = ?")
            .bind(&payload.hanja_eum)
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

    // Update sentences table
    let needs_sentence_validation = payload.sentence.is_some() || payload.target.is_some();
    
    if needs_sentence_validation {
        // Get current sentence and target
        let current = sqlx::query(
            "SELECT text, target FROM sentences WHERE card_id = ? LIMIT 1"
        )
        .bind(card_id)
        .fetch_one(&mut *tx)
        .await?;

        let sentence = payload.sentence.as_ref()
            .map(|s| s.as_str())
            .unwrap_or_else(|| current.get("text"));
        let target = payload.target.as_ref()
            .map(|t| t.as_str())
            .unwrap_or_else(|| current.get("target"));

        if sentence.trim().is_empty() {
            return Err(AppError::BadRequest("Sentence cannot be empty".to_string()));
        }
        if target.trim().is_empty() {
            return Err(AppError::BadRequest("Target cannot be empty".to_string()));
        }
        if !sentence.contains(target) {
            return Err(AppError::BadRequest(
                "Target word must appear in the sentence".to_string(),
            ));
        }
    }

    if let Some(sentence) = &payload.sentence {
        sqlx::query("UPDATE sentences SET text = ? WHERE card_id = ?")
            .bind(sentence)
            .bind(card_id)
            .execute(&mut *tx)
            .await?;
    }

    if let Some(target) = &payload.target {
        sqlx::query("UPDATE sentences SET target = ? WHERE card_id = ?")
            .bind(target)
            .bind(card_id)
            .execute(&mut *tx)
            .await?;
    }

    // Update sentence_translations
    if let Some(sentence_translation) = &payload.sentence_translation {
        if sentence_translation.trim().is_empty() {
            return Err(AppError::BadRequest("Sentence translation cannot be empty".to_string()));
        }
        // Get sentence_id first
        let sentence_id: i64 = sqlx::query_scalar(
            "SELECT id FROM sentences WHERE card_id = ? LIMIT 1"
        )
        .bind(card_id)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query("UPDATE sentence_translations SET translation = ? WHERE sentence_id = ?")
            .bind(sentence_translation)
            .bind(sentence_id)
            .execute(&mut *tx)
            .await?;
    }

    // Update sentence_inflection_hints
    if payload.speech_level.is_some() || payload.tense.is_some() {
        let sentence_id: i64 = sqlx::query_scalar(
            "SELECT id FROM sentences WHERE card_id = ? LIMIT 1"
        )
        .bind(card_id)
        .fetch_one(&mut *tx)
        .await?;

        // Check if hints exist
        let hints_exist: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM sentence_inflection_hints WHERE sentence_id = ?"
        )
        .bind(sentence_id)
        .fetch_one(&mut *tx)
        .await?;

        let speech_level_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "speech_levels", payload.speech_level.clone().map(Some)).await?.flatten();
        let tense_id: Option<i64> = crate::enum_lookup::resolve_optional_id(&mut tx, "tenses", payload.tense.clone().map(Some)).await?.flatten();

        if hints_exist {
            if payload.speech_level.is_some() {
                sqlx::query("UPDATE sentence_inflection_hints SET speech_level_id = ? WHERE sentence_id = ?")
                    .bind(speech_level_id)
                    .bind(sentence_id)
                    .execute(&mut *tx)
                    .await?;
            }

            if payload.tense.is_some() {
                sqlx::query("UPDATE sentence_inflection_hints SET tense_id = ? WHERE sentence_id = ?")
                    .bind(tense_id)
                    .bind(sentence_id)
                    .execute(&mut *tx)
                    .await?;
            }
        } else {
            // Insert new hints if either is provided (matches create_custom_card's behavior)
            sqlx::query(
                "INSERT INTO sentence_inflection_hints (sentence_id, speech_level_id, tense_id) VALUES (?, ?, ?)"
            )
            .bind(sentence_id)
            .bind(speech_level_id)
            .bind(tense_id)
            .execute(&mut *tx)
            .await?;
        }
    }

    // Update alternative targets
    if let Some(ref alts) = payload.alternatives {
        let sentence_id: i64 = sqlx::query_scalar(
            "SELECT id FROM sentences WHERE card_id = ? LIMIT 1"
        )
        .bind(card_id)
        .fetch_one(&mut *tx)
        .await?;

        // Delete existing
        sqlx::query("DELETE FROM sentence_alternative_targets WHERE sentence_id = ?")
            .bind(sentence_id)
            .execute(&mut *tx)
            .await?;

        // Insert new
        for alt in alts {
            let trimmed = alt.trim();
            if !trimmed.is_empty() {
                sqlx::query(
                    "INSERT INTO sentence_alternative_targets (sentence_id, alt_target) VALUES (?, ?)"
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