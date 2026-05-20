use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::{info, warn};

use crate::error::AppError;
use crate::auth::AuthUser;

// Export/Import data structures

#[derive(Serialize, Deserialize)]
pub struct ExportData {
    pub version: String,
    pub exported_at: String,
    pub settings: UserSettingsExport,
    pub card_states: Vec<CardStateExport>,
    pub review_history: Vec<ReviewHistoryExport>,
    pub suspended_cards: Vec<i64>,
    pub custom_cards: Vec<CustomCardExport>,
}

#[derive(Serialize, Deserialize)]
pub struct UserSettingsExport {
    pub show_percentage: bool,
    pub red_threshold: i64,
    pub yellow_threshold: i64,
    pub day_boundary_hour: i64,
    pub auto_progress_on_correct: bool,
    pub auto_progress_delay: i64,
    pub desired_retention: f64,
    pub daily_new_card_limit: i64,
}

#[derive(Serialize, Deserialize)]
pub struct CardStateExport {
    pub card_id: i64,
    pub stability: f64,
    pub difficulty: f64,
    pub last_review: Option<String>,
    pub state: String,
}

#[derive(Serialize, Deserialize)]
pub struct ReviewHistoryExport {
    pub card_id: i64,
    pub rating: String,
    pub scheduled_days: Option<f64>,
    pub elapsed_days: Option<f64>,
    pub reviewed_at: String,
    pub stability: Option<f64>,
    pub difficulty: Option<f64>,
    pub state: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct CustomCardExport {
    pub word: String,
    pub definition: Option<String>,
    pub pos: Option<String>,
    pub origin_type: Option<String>,
    pub hanja: Option<String>,
    pub hanja_eum: Option<String>,
    pub grade: Option<String>,
    pub translations: Vec<CardTranslationExport>,
    pub sentences: Vec<SentenceExport>,
}

#[derive(Serialize, Deserialize)]
pub struct CardTranslationExport {
    pub language_tag: String,
    pub trans_word: String,
    pub trans_dfn: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SentenceExport {
    pub text: String,
    pub target: String,
    #[serde(default)]
    pub alternatives: Vec<String>,
    pub translation: Option<String>,
    pub inflection_hint: Option<InflectionHintExport>,
}

#[derive(Serialize, Deserialize)]
pub struct InflectionHintExport {
    pub speech_level: String,
    pub tense: String,
}

#[derive(Deserialize)]
pub struct ImportDataRequest {
    pub data: ExportData,
    pub overwrite: bool,
}

#[derive(Serialize)]
pub struct ImportDataResponse {
    pub success: bool,
    pub message: String,
    pub stats: ImportStats,
}

#[derive(Serialize, Debug)]
pub struct ImportStats {
    pub card_states_imported: usize,
    pub reviews_imported: usize,
    pub suspended_cards_imported: usize,
    pub custom_cards_imported: usize,
}

// Export user data
pub async fn export_data(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<ExportData>, AppError> {
    let user_id = auth.0;
    info!("Exporting data for user_id: {}", user_id);

    // Get settings
    let settings = get_user_settings(&pool, user_id).await?;

    // Get card states
    let card_states_rows = sqlx::query(
        r#"
        SELECT card_id, stability, difficulty, last_review, state
        FROM card_states
        WHERE user_id = ?
        "#
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let card_states: Vec<CardStateExport> = card_states_rows.iter().map(|row| {
        CardStateExport {
            card_id: row.get("card_id"),
            stability: row.get("stability"),
            difficulty: row.get("difficulty"),
            last_review: row.get("last_review"),
            state: row.get("state"),
        }
    }).collect();

    // Get review history
    let review_history_rows = sqlx::query(
        r#"
        SELECT card_id, rating, scheduled_days, elapsed_days, reviewed_at, stability, difficulty, state
        FROM review_history
        WHERE user_id = ?
        ORDER BY reviewed_at ASC
        "#
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let review_history: Vec<ReviewHistoryExport> = review_history_rows.iter().map(|row| {
        ReviewHistoryExport {
            card_id: row.get("card_id"),
            rating: row.get("rating"),
            scheduled_days: row.get("scheduled_days"),
            elapsed_days: row.get("elapsed_days"),
            reviewed_at: row.get("reviewed_at"),
            stability: row.get("stability"),
            difficulty: row.get("difficulty"),
            state: row.get("state"),
        }
    }).collect();

    // Get suspended cards
    let suspended_cards: Vec<i64> = sqlx::query_scalar(
        r#"
        SELECT card_id
        FROM user_card_flags
        WHERE user_id = ? AND suspended = 1
        "#
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    // Get custom cards
    let custom_card_ids: Vec<i64> = sqlx::query_scalar(
        r#"
        SELECT card_id
        FROM custom_card_metadata
        WHERE user_id = ?
        "#
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let mut custom_cards = Vec::new();
    for card_id in custom_card_ids {
        // Get card info
        let card_row = sqlx::query(
            r#"
            SELECT word, definition, pos, origin_type, hanja, hanja_eum, grade
            FROM cards
            WHERE id = ?
            "#
        )
        .bind(card_id)
        .fetch_one(&pool)
        .await?;

        // Get translations
        let translation_rows = sqlx::query(
            r#"
            SELECT language_tag, trans_word, trans_dfn
            FROM card_translations
            WHERE card_id = ?
            "#
        )
        .bind(card_id)
        .fetch_all(&pool)
        .await?;

        let translations: Vec<CardTranslationExport> = translation_rows.iter().map(|row| {
            CardTranslationExport {
                language_tag: row.get("language_tag"),
                trans_word: row.get("trans_word"),
                trans_dfn: row.get("trans_dfn"),
            }
        }).collect();

        // Get sentences
        let sentence_rows = sqlx::query(
            r#"
            SELECT id, text, target
            FROM sentences
            WHERE card_id = ?
            "#
        )
        .bind(card_id)
        .fetch_all(&pool)
        .await?;

        let mut sentences = Vec::new();
        for sentence_row in sentence_rows {
            let sentence_id: i64 = sentence_row.get("id");
            
            // Get translation
            let translation: Option<String> = sqlx::query_scalar(
                "SELECT translation FROM sentence_translations WHERE sentence_id = ?"
            )
            .bind(sentence_id)
            .fetch_optional(&pool)
            .await?;

            // Get inflection hint
            let inflection_hint_row = sqlx::query(
                "SELECT speech_level, tense FROM sentence_inflection_hints WHERE sentence_id = ?"
            )
            .bind(sentence_id)
            .fetch_optional(&pool)
            .await?;

            let inflection_hint = inflection_hint_row.map(|row| {
                InflectionHintExport {
                    speech_level: row.get("speech_level"),
                    tense: row.get("tense"),
                }
            });

            // Get alternatives
            let alternatives: Vec<String> = sqlx::query_scalar(
                "SELECT alt_target FROM sentence_alternative_targets WHERE sentence_id = ?"
            )
            .bind(sentence_id)
            .fetch_all(&pool)
            .await?;

            sentences.push(SentenceExport {
                text: sentence_row.get("text"),
                target: sentence_row.get("target"),
                alternatives,
                translation,
                inflection_hint,
            });
        }

        custom_cards.push(CustomCardExport {
            word: card_row.get("word"),
            definition: card_row.get("definition"),
            pos: card_row.get("pos"),
            origin_type: card_row.get("origin_type"),
            hanja: card_row.get("hanja"),
            hanja_eum: card_row.get("hanja_eum"),
            grade: card_row.get("grade"),
            translations,
            sentences,
        });
    }

    let export_data = ExportData {
        version: "1.0".to_string(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        settings,
        card_states,
        review_history,
        suspended_cards,
        custom_cards,
    };

    info!("Export complete: {} card states, {} reviews, {} suspended cards, {} custom cards",
        export_data.card_states.len(),
        export_data.review_history.len(),
        export_data.suspended_cards.len(),
        export_data.custom_cards.len()
    );

    Ok(Json(export_data))
}

// Import user data
pub async fn import_data(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Json(payload): Json<ImportDataRequest>,
) -> Result<Json<ImportDataResponse>, AppError> {
    let user_id = auth.0;
    info!("Importing data for user_id: {} (overwrite: {})", user_id, payload.overwrite);

    let data = payload.data;

    // Validate version
    if data.version != "1.0" {
        return Err(AppError::Internal(format!("Unsupported export version: {}", data.version)));
    }

    // Begin transaction
    let mut tx = pool.begin().await?;

    // If overwrite, delete existing data
    if payload.overwrite {
        info!("Overwrite enabled - clearing existing data");
        
        sqlx::query("DELETE FROM review_history WHERE user_id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        
        sqlx::query("DELETE FROM card_states WHERE user_id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        
        sqlx::query("DELETE FROM user_card_flags WHERE user_id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        // Delete custom cards (cascade will handle related tables)
        let custom_card_ids: Vec<i64> = sqlx::query_scalar(
            "SELECT card_id FROM custom_card_metadata WHERE user_id = ?"
        )
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;

        for card_id in custom_card_ids {
            sqlx::query("DELETE FROM cards WHERE id = ?")
                .bind(card_id)
                .execute(&mut *tx)
                .await?;
        }
    }

    let mut stats = ImportStats {
        card_states_imported: 0,
        reviews_imported: 0,
        suspended_cards_imported: 0,
        custom_cards_imported: 0,
    };

    // Import custom cards first (so we have valid card_ids for card_states)
    for custom_card in data.custom_cards {
        // Insert card
        let card_id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO cards (word, definition, pos, origin_type, hanja, hanja_eum, grade)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            RETURNING id
            "#
        )
        .bind(&custom_card.word)
        .bind(&custom_card.definition)
        .bind(&custom_card.pos)
        .bind(&custom_card.origin_type)
        .bind(&custom_card.hanja)
        .bind(&custom_card.hanja_eum)
        .bind(&custom_card.grade)
        .fetch_one(&mut *tx)
        .await?;

        // Insert custom_card_metadata
        sqlx::query(
            "INSERT INTO custom_card_metadata (card_id, user_id) VALUES (?, ?)"
        )
        .bind(card_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        // Insert translations
        for translation in custom_card.translations {
            sqlx::query(
                r#"
                INSERT INTO card_translations (card_id, language_tag, trans_word, trans_dfn)
                VALUES (?, ?, ?, ?)
                "#
            )
            .bind(card_id)
            .bind(&translation.language_tag)
            .bind(&translation.trans_word)
            .bind(&translation.trans_dfn)
            .execute(&mut *tx)
            .await?;
        }

        // Insert sentences
        for sentence in custom_card.sentences {
            let sentence_id = sqlx::query_scalar::<_, i64>(
                r#"
                INSERT INTO sentences (card_id, text, target)
                VALUES (?, ?, ?)
                RETURNING id
                "#
            )
            .bind(card_id)
            .bind(&sentence.text)
            .bind(&sentence.target)
            .fetch_one(&mut *tx)
            .await?;

            // Insert sentence translation if present
            if let Some(translation) = sentence.translation {
                sqlx::query(
                    "INSERT INTO sentence_translations (sentence_id, translation) VALUES (?, ?)"
                )
                .bind(sentence_id)
                .bind(&translation)
                .execute(&mut *tx)
                .await?;
            }

            // Insert inflection hint if present
            if let Some(hint) = sentence.inflection_hint {
                sqlx::query(
                    r#"
                    INSERT INTO sentence_inflection_hints (sentence_id, speech_level, tense)
                    VALUES (?, ?, ?)
                    "#
                )
                .bind(sentence_id)
                .bind(&hint.speech_level)
                .bind(&hint.tense)
                .execute(&mut *tx)
                .await?;
            }

            // Insert alternatives
            for alt in &sentence.alternatives {
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

        stats.custom_cards_imported += 1;
    }

    // Import card states
    for card_state in data.card_states {
        // Check if card exists
        let card_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM cards WHERE id = ?)"
        )
        .bind(card_state.card_id)
        .fetch_one(&mut *tx)
        .await?;

        if !card_exists {
            warn!("Skipping card_state for non-existent card_id: {}", card_state.card_id);
            continue;
        }

        sqlx::query(
            r#"
            INSERT INTO card_states (card_id, user_id, stability, difficulty, last_review, state)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(card_id, user_id) DO UPDATE SET
                stability = excluded.stability,
                difficulty = excluded.difficulty,
                last_review = excluded.last_review,
                state = excluded.state
            "#
        )
        .bind(card_state.card_id)
        .bind(user_id)
        .bind(card_state.stability)
        .bind(card_state.difficulty)
        .bind(card_state.last_review)
        .bind(card_state.state)
        .execute(&mut *tx)
        .await?;

        stats.card_states_imported += 1;
    }

    // Import review history
    for review in data.review_history {
        // Check if card exists
        let card_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM cards WHERE id = ?)"
        )
        .bind(review.card_id)
        .fetch_one(&mut *tx)
        .await?;

        if !card_exists {
            warn!("Skipping review for non-existent card_id: {}", review.card_id);
            continue;
        }

        sqlx::query(
            r#"
            INSERT INTO review_history (card_id, user_id, rating, scheduled_days, elapsed_days, reviewed_at, stability, difficulty, state)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(review.card_id)
        .bind(user_id)
        .bind(review.rating)
        .bind(review.scheduled_days)
        .bind(review.elapsed_days)
        .bind(review.reviewed_at)
        .bind(review.stability)
        .bind(review.difficulty)
        .bind(review.state)
        .execute(&mut *tx)
        .await?;

        stats.reviews_imported += 1;
    }

    // Import suspended cards
    for card_id in data.suspended_cards {
        // Check if card exists
        let card_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM cards WHERE id = ?)"
        )
        .bind(card_id)
        .fetch_one(&mut *tx)
        .await?;

        if !card_exists {
            warn!("Skipping suspension for non-existent card_id: {}", card_id);
            continue;
        }

        sqlx::query(
            r#"
            INSERT INTO user_card_flags (user_id, card_id, suspended)
            VALUES (?, ?, 1)
            ON CONFLICT(user_id, card_id) DO UPDATE SET suspended = 1
            "#
        )
        .bind(user_id)
        .bind(card_id)
        .execute(&mut *tx)
        .await?;

        stats.suspended_cards_imported += 1;
    }

    // Import settings
    sqlx::query(
        r#"
        INSERT INTO user_settings (user_id, show_percentage, red_threshold, yellow_threshold, 
                                   day_boundary_hour, auto_progress_on_correct, auto_progress_delay, 
                                   desired_retention, daily_new_card_limit)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
            show_percentage = excluded.show_percentage,
            red_threshold = excluded.red_threshold,
            yellow_threshold = excluded.yellow_threshold,
            day_boundary_hour = excluded.day_boundary_hour,
            auto_progress_on_correct = excluded.auto_progress_on_correct,
            auto_progress_delay = excluded.auto_progress_delay,
            desired_retention = excluded.desired_retention,
            daily_new_card_limit = excluded.daily_new_card_limit
        "#
    )
    .bind(user_id)
    .bind(data.settings.show_percentage)
    .bind(data.settings.red_threshold)
    .bind(data.settings.yellow_threshold)
    .bind(data.settings.day_boundary_hour)
    .bind(data.settings.auto_progress_on_correct)
    .bind(data.settings.auto_progress_delay)
    .bind(data.settings.desired_retention)
    .bind(data.settings.daily_new_card_limit)
    .execute(&mut *tx)
    .await?;

    // Commit transaction
    tx.commit().await?;

    info!("Import complete: {:?}", stats);

    Ok(Json(ImportDataResponse {
        success: true,
        message: "Data imported successfully".to_string(),
        stats,
    }))
}

// Helper function to get user settings
async fn get_user_settings(pool: &SqlitePool, user_id: i64) -> Result<UserSettingsExport, AppError> {
    // Ensure user_settings row exists
    sqlx::query(
        r#"
        INSERT INTO user_settings (user_id)
        VALUES (?)
        ON CONFLICT(user_id) DO NOTHING
        "#
    )
    .bind(user_id)
    .execute(pool)
    .await?;

    let row = sqlx::query(
        r#"
        SELECT show_percentage, red_threshold, yellow_threshold, day_boundary_hour, 
               auto_progress_on_correct, auto_progress_delay, desired_retention, daily_new_card_limit
        FROM user_settings
        WHERE user_id = ?
        "#
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(UserSettingsExport {
        show_percentage: row.get("show_percentage"),
        red_threshold: row.get("red_threshold"),
        yellow_threshold: row.get("yellow_threshold"),
        day_boundary_hour: row.get("day_boundary_hour"),
        auto_progress_on_correct: row.get("auto_progress_on_correct"),
        auto_progress_delay: row.get("auto_progress_delay"),
        desired_retention: row.get("desired_retention"),
        daily_new_card_limit: row.get("daily_new_card_limit"),
    })
}