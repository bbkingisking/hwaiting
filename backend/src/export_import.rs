use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::{info, warn};
use utoipa::ToSchema;

use crate::error::{AppError, AppJson};
use crate::auth::AuthUser;

// Export/Import data structures

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ExportData {
    pub version: String,
    pub exported_at: String,
    pub settings: UserSettingsExport,
    pub review_history: Vec<ReviewHistoryExport>,
    pub suppressed_cards: Vec<i64>,
    pub custom_cards: Vec<CustomCardExport>,
}

// UserSettingsCore (user.rs) is the `user_settings` row proper, flattened
// here plus the one field genuinely specific to export/import:
// `fsrs_parameters` carries the actual fitted parameters so they round-trip
// through an export, where `user::UserSettings` only exposes whether they're
// set (see UserSettingsCore's doc comment for why this used to be a second
// hand-declared copy of the same 11 fields).
#[derive(Serialize, Deserialize, ToSchema)]
pub struct UserSettingsExport {
    #[serde(flatten)]
    pub core: crate::user::UserSettingsCore,
    pub fsrs_parameters: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
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

#[derive(Serialize, Deserialize, ToSchema)]
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

#[derive(Serialize, Deserialize, ToSchema)]
pub struct CardTranslationExport {
    pub language_tag: String,
    pub trans_word: String,
    pub trans_dfn: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct SentenceExport {
    pub text: String,
    pub target: String,
    #[serde(default)]
    pub alternatives: Vec<String>,
    pub translation: Option<String>,
    pub inflection_hint: Option<InflectionHintExport>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct InflectionHintExport {
    pub speech_level: Option<String>,
    pub tense: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct ImportDataRequest {
    pub data: ExportData,
    pub overwrite: bool,
}

#[derive(Serialize, ToSchema)]
pub struct ImportDataResponse {
    pub success: bool,
    pub message: String,
    pub stats: ImportStats,
}

#[derive(Serialize, Debug, ToSchema)]
pub struct ImportStats {
    pub card_states_derived: usize,
    pub reviews_imported: usize,
    pub suppressed_cards_imported: usize,
    pub custom_cards_imported: usize,
}

// Export user data
#[utoipa::path(
    get,
    path = "/api/user/export",
    responses(
        (status = 200, description = "Full data export: settings, review history, suppressed cards, custom cards", body = ExportData),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "user"
)]
pub async fn export_data(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<ExportData>, AppError> {
    let user_id = auth.0;
    info!("Exporting data for user_id: {}", user_id);

    // Get settings
    let settings = get_user_settings(&pool, user_id).await?;

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

    // Get suppressed cards
    let suppressed_cards: Vec<i64> = sqlx::query_scalar(
        r#"
        SELECT card_id
        FROM user_card_flags
        WHERE user_id = ? AND suppressed = 1
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
            SELECT c.word, c.definition, pop.slug as pos, ot.slug as origin_type,
                   c.hanja, c.hanja_eum, g.slug as grade
            FROM cards c
            LEFT JOIN parts_of_speech pop ON pop.id = c.pos_id
            LEFT JOIN origin_types ot ON ot.id = c.origin_type_id
            LEFT JOIN grades g ON g.id = c.grade_id
            WHERE c.id = ?
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
                r#"
                SELECT sl.slug as speech_level, tn.slug as tense
                FROM sentence_inflection_hints sih
                LEFT JOIN speech_levels sl ON sl.id = sih.speech_level_id
                LEFT JOIN tenses tn ON tn.id = sih.tense_id
                WHERE sih.sentence_id = ?
                "#
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
        review_history,
        suppressed_cards,
        custom_cards,
    };

    info!("Export complete: {} reviews, {} suppressed cards, {} custom cards",
        export_data.review_history.len(),
        export_data.suppressed_cards.len(),
        export_data.custom_cards.len()
    );

    Ok(Json(export_data))
}

// Import user data
#[utoipa::path(
    post,
    path = "/api/user/import",
    request_body = ImportDataRequest,
    responses(
        (status = 200, description = "Data imported (card_states derived from imported review_history)", body = ImportDataResponse),
        (status = 400, description = "Unsupported export version or malformed request", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "user"
)]
pub async fn import_data(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    AppJson(payload): AppJson<ImportDataRequest>,
) -> Result<Json<ImportDataResponse>, AppError> {
    let user_id = auth.0;
    info!("Importing data for user_id: {} (overwrite: {})", user_id, payload.overwrite);

    let data = payload.data;

    // Validate version
    if data.version != "1.0" {
        return Err(AppError::BadRequest(format!("Unsupported export version: {}", data.version)));
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
        card_states_derived: 0,
        reviews_imported: 0,
        suppressed_cards_imported: 0,
        custom_cards_imported: 0,
    };

    // Import custom cards first (so we have valid card_ids for card_states)
    for custom_card in data.custom_cards {
        // Resolve enum slugs -> lookup table ids, auto-registering any slug that
        // doesn't already exist (e.g. from an older export) so import never fails
        // just because of a stale/unknown enum value.
        let pos_id = crate::enum_lookup::resolve_or_create_id(&mut tx, "parts_of_speech", custom_card.pos.clone()).await?;
        let origin_type_id = crate::enum_lookup::resolve_or_create_id(&mut tx, "origin_types", custom_card.origin_type.clone()).await?;
        let grade_id = crate::enum_lookup::resolve_or_create_id(&mut tx, "grades", custom_card.grade.clone()).await?;

        // Insert card
        let card_id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO cards (word, definition, pos_id, origin_type_id, hanja, hanja_eum, grade_id)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            RETURNING id
            "#
        )
        .bind(&custom_card.word)
        .bind(&custom_card.definition)
        .bind(pos_id)
        .bind(origin_type_id)
        .bind(&custom_card.hanja)
        .bind(&custom_card.hanja_eum)
        .bind(grade_id)
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
                let speech_level_id = crate::enum_lookup::resolve_or_create_id(&mut tx, "speech_levels", hint.speech_level).await?;
                let tense_id = crate::enum_lookup::resolve_or_create_id(&mut tx, "tenses", hint.tense).await?;
                sqlx::query(
                    r#"
                    INSERT INTO sentence_inflection_hints (sentence_id, speech_level_id, tense_id)
                    VALUES (?, ?, ?)
                    "#
                )
                .bind(sentence_id)
                .bind(speech_level_id)
                .bind(tense_id)
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

    // Import review history (must come before card_states derivation)
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

    // Derive card_states from the last review_history entry per card
    let derived_result = sqlx::query(
        r#"
        INSERT INTO card_states (card_id, user_id, stability, difficulty, last_review, state)
        SELECT rh.card_id, ?, rh.stability, rh.difficulty, rh.reviewed_at, rh.state
        FROM review_history rh
        INNER JOIN (
            SELECT card_id, MAX(reviewed_at) AS max_reviewed
            FROM review_history
            WHERE user_id = ?
            GROUP BY card_id
        ) latest ON rh.card_id = latest.card_id AND rh.reviewed_at = latest.max_reviewed
        WHERE rh.user_id = ? AND rh.stability IS NOT NULL
        ON CONFLICT(card_id, user_id) DO UPDATE SET
            stability = excluded.stability,
            difficulty = excluded.difficulty,
            last_review = excluded.last_review,
            state = excluded.state
        "#
    )
    .bind(user_id)
    .bind(user_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    stats.card_states_derived = derived_result.rows_affected() as usize;

    // Import suppressed cards
    for card_id in data.suppressed_cards {
        // Check if card exists
        let card_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM cards WHERE id = ?)"
        )
        .bind(card_id)
        .fetch_one(&mut *tx)
        .await?;

        if !card_exists {
            warn!("Skipping suppression for non-existent card_id: {}", card_id);
            continue;
        }

        sqlx::query(
            r#"
            INSERT INTO user_card_flags (user_id, card_id, suppressed)
            VALUES (?, ?, 1)
            ON CONFLICT(user_id, card_id) DO UPDATE SET suppressed = 1
            "#
        )
        .bind(user_id)
        .bind(card_id)
        .execute(&mut *tx)
        .await?;

        stats.suppressed_cards_imported += 1;
    }

    // Import settings
    sqlx::query(
        r#"
        INSERT INTO user_settings (user_id, show_percentage, red_threshold, yellow_threshold,
                                   day_boundary_hour, auto_progress_on_correct, auto_progress_delay,
                                   desired_retention, daily_new_card_limit,
                                   history_colorized_area, history_colored_dots, history_threshold_lines)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
            show_percentage = excluded.show_percentage,
            red_threshold = excluded.red_threshold,
            yellow_threshold = excluded.yellow_threshold,
            day_boundary_hour = excluded.day_boundary_hour,
            auto_progress_on_correct = excluded.auto_progress_on_correct,
            auto_progress_delay = excluded.auto_progress_delay,
            desired_retention = excluded.desired_retention,
            daily_new_card_limit = excluded.daily_new_card_limit,
            history_colorized_area = excluded.history_colorized_area,
            history_colored_dots = excluded.history_colored_dots,
            history_threshold_lines = excluded.history_threshold_lines
        "#
    )
    .bind(user_id)
    .bind(data.settings.core.show_percentage)
    .bind(data.settings.core.red_threshold)
    .bind(data.settings.core.yellow_threshold)
    .bind(data.settings.core.day_boundary_hour)
    .bind(data.settings.core.auto_progress_on_correct)
    .bind(data.settings.core.auto_progress_delay)
    .bind(data.settings.core.desired_retention)
    .bind(data.settings.core.daily_new_card_limit)
    .bind(data.settings.core.history_colorized_area)
    .bind(data.settings.core.history_colored_dots)
    .bind(data.settings.core.history_threshold_lines)
    .execute(&mut *tx)
    .await?;

    // Import FSRS parameters if present
    if let Some(ref fsrs_params) = data.settings.fsrs_parameters {
        sqlx::query(
            r#"
            INSERT INTO user_fsrs_parameters (user_id, parameters)
            VALUES (?, ?)
            ON CONFLICT(user_id) DO UPDATE SET parameters = excluded.parameters
            "#
        )
        .bind(user_id)
        .bind(fsrs_params)
        .execute(&mut *tx)
        .await?;
    }

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

    let core = sqlx::query_as::<_, crate::user::UserSettingsCore>(
        r#"
        SELECT show_percentage, red_threshold, yellow_threshold, day_boundary_hour,
               auto_progress_on_correct, auto_progress_delay, desired_retention, daily_new_card_limit,
               history_colorized_area, history_colored_dots, history_threshold_lines
        FROM user_settings
        WHERE user_id = ?
        "#
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    // Get FSRS parameters if they exist
    let fsrs_parameters: Option<String> = sqlx::query_scalar(
        "SELECT parameters FROM user_fsrs_parameters WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(UserSettingsExport { core, fsrs_parameters })
}