use axum::{Json, extract::State};
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::{info, warn};
use utoipa::ToSchema;

use crate::auth::AuthUser;
use crate::error::{AppError, AppJson};

// Schema vocabularies for the strings retained in historical backup records.
#[derive(ToSchema)]
#[allow(dead_code)]
enum BackupVersion {
    #[serde(rename = "1.0")]
    V1,
}

#[derive(ToSchema)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
enum ReviewRating {
    Again,
    Hard,
    Good,
    Easy,
}

#[derive(ToSchema)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
enum ReviewState {
    Learning,
    Review,
    Relearning,
}

// Export/Import data structures

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ExportData {
    #[schema(value_type = BackupVersion)]
    pub version: String,
    /// RFC 3339 UTC. Import also accepts SQLite YYYY-MM-DD HH:MM:SS.
    #[schema(format = DateTime)]
    pub exported_at: String,
    pub settings: UserSettingsExport,
    pub review_history: Vec<ReviewHistoryExport>,
    pub suppressed_cards: Vec<i64>,
}

// UserSettingsCore (user.rs) is the `users_settings` row proper, flattened
// here plus the one field genuinely specific to export/import:
// `fsrs_parameters` carries the actual fitted parameters so they round-trip
// through an export, where `user::UserSettings` only exposes whether they're
// set (see UserSettingsCore's doc comment for why this used to be a second
// hand-declared copy of the same settings fields).
#[derive(Serialize, Deserialize, ToSchema)]
pub struct UserSettingsExport {
    #[serde(flatten)]
    pub core: crate::user::UserSettingsCore,
    /// Fitted FSRS parameters, or null when the library defaults are in use. On import,
    /// null resets to defaults with overwrite=true and preserves existing parameters otherwise.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schema(required)]
    pub fsrs_parameters: Option<Vec<f32>>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ReviewHistoryExport {
    pub card_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentence_id: Option<i64>,
    #[schema(value_type = ReviewRating)]
    pub rating: String,
    #[schema(minimum = 0)]
    pub scheduled_days: Option<f64>,
    #[schema(minimum = 0)]
    pub elapsed_days: Option<f64>,
    /// Review time, RFC 3339 UTC. Import also accepts SQLite YYYY-MM-DD HH:MM:SS.
    #[schema(format = DateTime)]
    pub reviewed_at: String,
    #[schema(exclusive_minimum = 0, maximum = 3.4028234663852886e38)]
    /// Positive FSRS stability in days, supplied together with difficulty, or both null.
    pub stability: Option<f64>,
    #[schema(minimum = 1, maximum = 10)]
    /// Required together with stability; state must also be supplied when both are present.
    pub difficulty: Option<f64>,
    #[schema(value_type = Option<ReviewState>)]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrong_answer: Option<WrongAnswerExport>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct WrongAnswerExport {
    pub target_sentence_id: i64,
    pub answer: String,
}

#[derive(Deserialize, ToSchema)]
pub struct ImportDataRequest {
    pub data: ExportData,
    /// Clear existing review history, derived scheduling, suppression flags, and fitted FSRS
    /// parameters before import. Settings are applied from the backup in either mode;
    /// null timezone/language values preserve existing saved choices.
    pub overwrite: bool,
}

#[derive(Serialize, ToSchema)]
pub struct ImportDataResponse {
    pub stats: ImportStats,
}

#[derive(Serialize, Debug, ToSchema)]
pub struct ImportStats {
    #[schema(value_type = u64)]
    pub card_states_derived: usize,
    #[schema(value_type = u64)]
    pub reviews_imported: usize,
    #[schema(value_type = u64)]
    pub suppressed_cards_imported: usize,
}

/// Validate backup-owned values before opening the replacement transaction.
/// Nullable metadata from older exports remains valid history, but any
/// supplied memory state must be a usable FSRS state. The final review for
/// each restored card is checked separately after merging with existing data.
fn validate_import_data(data: &ExportData) -> Result<(), AppError> {
    if data.version != "1.0" {
        return Err(AppError::BadRequest(format!(
            "Unsupported export version: {}",
            data.version
        )));
    }
    crate::user::validate_settings_values(
        Some(data.settings.core.red_threshold),
        Some(data.settings.core.yellow_threshold),
        Some(data.settings.core.day_boundary_hour),
        Some(data.settings.core.auto_progress_delay),
        Some(data.settings.core.desired_retention),
        Some(data.settings.core.daily_new_card_limit),
    )?;
    crate::user::parse_study_timezone(data.settings.core.study_timezone.as_deref())?;
    if let Some(code) = &data.settings.core.translation_language {
        crate::translation_language::validate_code(code)?;
    }

    // Construct the actual model; parsing the JSON alone does not establish
    // that its parameter count and values can schedule a card.
    crate::cards::scheduling::Scheduler::from_parameters(
        data.settings.fsrs_parameters.as_deref(),
        data.settings.core.desired_retention,
    )?;

    let exported_at = crate::cards::parse_flexible_datetime(&data.exported_at)
        .map_err(|e| AppError::BadRequest(format!("Invalid export timestamp: {e}")))?;
    if !(1..=9999).contains(&exported_at.year()) {
        return Err(AppError::BadRequest(
            "Export timestamp is out of range".into(),
        ));
    }

    for (index, review) in data.review_history.iter().enumerate() {
        let field = |message: &str| AppError::BadRequest(format!("Review {index}: {message}"));
        if !matches!(review.rating.as_str(), "again" | "hard" | "good" | "easy") {
            return Err(field("invalid rating"));
        }
        let reviewed_at = crate::cards::parse_flexible_datetime(&review.reviewed_at)
            .map_err(|_| field("invalid reviewed_at timestamp"))?;
        if !(1..=9999).contains(&reviewed_at.year()) {
            return Err(field("reviewed_at is out of range"));
        }
        if let Some(days) = review.scheduled_days
            && (!days.is_finite() || days < 0.0)
        {
            return Err(field("scheduled_days must be finite and nonnegative"));
        }
        if let Some(days) = review.elapsed_days
            && (!days.is_finite() || days < 0.0)
        {
            return Err(field("elapsed_days must be finite and nonnegative"));
        }
        if let Some(state) = review.state.as_deref()
            && !matches!(state, "learning" | "review" | "relearning")
        {
            return Err(field("invalid state"));
        }
        match (review.stability, review.difficulty) {
            (None, None) => {}
            (Some(stability), Some(difficulty)) => {
                if !stability.is_finite() || stability <= 0.0 || stability > f32::MAX as f64 {
                    return Err(field(
                        "stability must be positive and representable by FSRS",
                    ));
                }
                if !difficulty.is_finite() || !(1.0..=10.0).contains(&difficulty) {
                    return Err(field("difficulty must be between 1 and 10"));
                }
                if review.state.is_none() {
                    return Err(field("state is required when memory state is present"));
                }
            }
            _ => {
                return Err(field(
                    "stability and difficulty must both be present or absent",
                ));
            }
        }
    }
    Ok(())
}

// Export user data
#[utoipa::path(
    get,
    path = "/api/user/export",
    description = "Export the current user's settings, personal FSRS parameters, review history including recorded wrong answers, and suppressed cards as a versioned backup.",
    responses(
        (status = 200, description = "Full data export: settings, review history, suppressed cards", body = ExportData),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
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
        SELECT rh.card_id, rh.sentence_id, rh.rating, rh.scheduled_days, rh.elapsed_days,
               rh.reviewed_at, rh.stability, rh.difficulty, rh.state,
               wa.target_sentence_id, wa.answer
        FROM review_history rh
        LEFT JOIN wrong_answers wa ON wa.review_id = rh.id
        WHERE rh.user_id = ?
        ORDER BY rh.reviewed_at ASC, rh.id ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let review_history: Vec<ReviewHistoryExport> = review_history_rows
        .iter()
        .map(|row| -> Result<ReviewHistoryExport, AppError> {
            let reviewed_at: String = row.get("reviewed_at");
            let reviewed_at = crate::cards::parse_flexible_datetime(&reviewed_at)
                .map_err(|e| AppError::Internal(format!("Stored reviewed_at is invalid: {e}")))?;
            Ok(ReviewHistoryExport {
                card_id: row.get("card_id"),
                sentence_id: row.get("sentence_id"),
                rating: row.get("rating"),
                scheduled_days: row.get("scheduled_days"),
                elapsed_days: row.get("elapsed_days"),
                reviewed_at: crate::cards::rfc3339(reviewed_at),
                stability: row.get("stability"),
                difficulty: row.get("difficulty"),
                state: row.get("state"),
                wrong_answer: row.get::<Option<i64>, _>("target_sentence_id").map(
                    |target_sentence_id| WrongAnswerExport {
                        target_sentence_id,
                        answer: row.get("answer"),
                    },
                ),
            })
        })
        .collect::<Result<_, _>>()?;

    // Get suppressed cards
    let suppressed_cards: Vec<i64> = sqlx::query_scalar(
        r#"
        SELECT card_id
        FROM users_card_flags
        WHERE user_id = ? AND suppressed = 1
        "#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let export_data = ExportData {
        version: "1.0".to_string(),
        exported_at: crate::cards::rfc3339(chrono::Utc::now()),
        settings,
        review_history,
        suppressed_cards,
    };

    info!(
        "Export complete: {} reviews, {} suppressed cards",
        export_data.review_history.len(),
        export_data.suppressed_cards.len(),
    );

    Ok(Json(export_data))
}

// Import user data
#[utoipa::path(
    post,
    path = "/api/user/import",
    description = "Import a versioned backup of settings, review history, FSRS parameters, and suppressed cards in one transaction, rebuilding card state and scheduling. With overwrite enabled, clears existing review and suppression data first; otherwise imports into existing data. Previously displayed cards may become stale.",
    request_body = ImportDataRequest,
    responses(
        (status = 200, description = "Data imported (cards_states derived from imported review_history)", body = ImportDataResponse),
        (status = 400, description = "Invalid backup version, settings, FSRS model, review history, or malformed request", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "user"
)]
pub async fn import_data(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    AppJson(payload): AppJson<ImportDataRequest>,
) -> Result<Json<ImportDataResponse>, AppError> {
    let user_id = auth.0;
    info!(
        "Importing data for user_id: {} (overwrite: {})",
        user_id, payload.overwrite
    );

    let data = payload.data;

    validate_import_data(&data)?;
    if let Some(code) = &data.settings.core.translation_language {
        crate::translation_language::TranslationLanguage(code.clone())
            .id(&pool)
            .await?;
    }

    // Begin transaction
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    // If overwrite, delete existing data
    if payload.overwrite {
        info!("Overwrite enabled - clearing existing data");

        // wrong_answers.review_id has no ON DELETE CASCADE, so clear these
        // dependent rows before deleting their reviews.
        sqlx::query(
            "DELETE FROM wrong_answers WHERE review_id IN
             (SELECT id FROM review_history WHERE user_id = ?)",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM review_history WHERE user_id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        // Keep revision tombstones so an old card cannot become valid again
        // after an overwrite removes this card's review history.
        sqlx::query(
            "UPDATE cards_states SET stability = 0, difficulty = 0, last_review = NULL,
             state = 'new', due_at = NULL, review_revision = review_revision + 1 WHERE user_id = ?",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM users_card_flags WHERE user_id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM users_fsrs_parameters WHERE user_id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    }

    let mut stats = ImportStats {
        card_states_derived: 0,
        reviews_imported: 0,
        suppressed_cards_imported: 0,
    };

    // Every card id referenced below (across both the review-history and
    // suppressed-cards loops) used to get its own `SELECT EXISTS` query, one
    // round trip per imported row. Loading the whole id set once instead
    // turns that into a single query no matter how large the import is, with
    // an in-memory lookup replacing each per-row check.
    let valid_card_ids: std::collections::HashSet<i64> = sqlx::query_scalar("SELECT id FROM cards")
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect();
    let valid_target_cards: std::collections::HashMap<i64, i64> = sqlx::query(
        "SELECT s.id AS target_sentence_id, s.card_id
         FROM targets t JOIN sentences s ON s.id = t.sentence_id",
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|row| (row.get("target_sentence_id"), row.get("card_id")))
    .collect();
    let sole_sentence_by_card: std::collections::HashMap<i64, i64> = sqlx::query(
        "SELECT card_id, MIN(id) AS sentence_id FROM sentences GROUP BY card_id HAVING COUNT(*) = 1",
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|row| (row.get("card_id"), row.get("sentence_id")))
    .collect();

    // Import review history (must come before cards_states derivation)
    for review in data.review_history {
        if !valid_card_ids.contains(&review.card_id) {
            warn!(
                "Skipping review for non-existent card_id: {}",
                review.card_id
            );
            continue;
        }

        // Older exports have no sentence_id. A wrong answer identifies its
        // target sentence; otherwise infer only when the card has one.
        let proposed_sentence = review
            .sentence_id
            .or_else(|| review.wrong_answer.as_ref().map(|w| w.target_sentence_id))
            .or_else(|| sole_sentence_by_card.get(&review.card_id).copied());
        // A missing identity is valid for old exports of multi-sentence
        // cards. An identity supplied by the backup must resolve to this
        // card; silently replacing it with NULL loses review history.
        if let Some(sentence_id) = proposed_sentence
            && valid_target_cards.get(&sentence_id) != Some(&review.card_id)
        {
            return Err(AppError::BadRequest(format!(
                "Imported review for card {} has invalid sentence_id {}",
                review.card_id, sentence_id
            )));
        }
        let sentence_id = proposed_sentence;

        if let Some(ref wrong_answer) = review.wrong_answer
            && sentence_id != Some(wrong_answer.target_sentence_id)
        {
            return Err(AppError::BadRequest(format!(
                "Imported wrong answer for card {} does not match its sentence_id",
                review.card_id
            )));
        }

        let review_id = sqlx::query(
            r#"
            INSERT INTO review_history (card_id, sentence_id, user_id, rating, scheduled_days, elapsed_days, reviewed_at, stability, difficulty, state)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(review.card_id)
        .bind(sentence_id)
        .bind(user_id)
        .bind(review.rating)
        .bind(review.scheduled_days)
        .bind(review.elapsed_days)
        .bind(crate::cards::sqlite_datetime(
            crate::cards::parse_flexible_datetime(&review.reviewed_at)
                .map_err(|e| AppError::BadRequest(format!("invalid reviewed_at: {e}")))?,
        ))
        .bind(review.stability)
        .bind(review.difficulty)
        .bind(review.state)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();

        if let Some(wrong_answer) = review.wrong_answer {
            sqlx::query(
                "INSERT INTO wrong_answers (review_id, target_sentence_id, answer)
                 VALUES (?, ?, ?)",
            )
            .bind(review_id)
            .bind(wrong_answer.target_sentence_id)
            .bind(wrong_answer.answer)
            .execute(&mut *tx)
            .await?;
        }

        stats.reviews_imported += 1;
    }

    // Compare parsed instants, not timestamp text or SQLite's julianday
    // (which loses submillisecond precision). The row id breaks exact ties.
    let history_rows = sqlx::query(
        "SELECT id, card_id, stability, difficulty, reviewed_at, state
         FROM review_history WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    let mut latest_by_card = std::collections::HashMap::new();
    for row in history_rows {
        let card_id: i64 = row.get("card_id");
        let id: i64 = row.get("id");
        let reviewed_at: String = row.get("reviewed_at");
        let instant = crate::cards::parse_flexible_datetime(&reviewed_at).map_err(|_| {
            AppError::BadRequest(format!("Review for card {card_id} has invalid timestamp"))
        })?;
        let key = (instant, id);
        if latest_by_card
            .get(&card_id)
            .is_some_and(|(current, _)| *current >= key)
        {
            continue;
        }
        latest_by_card.insert(card_id, (key, row));
    }
    for (card_id, (_, row)) in latest_by_card {
        let stability: Option<f64> = row.get("stability");
        let difficulty: Option<f64> = row.get("difficulty");
        let state: Option<String> = row.get("state");
        let reviewed_at: String = row.get("reviewed_at");
        let (Some(stability), Some(difficulty), Some(state)) = (stability, difficulty, state)
        else {
            return Err(AppError::BadRequest(format!(
                "Latest review for card {card_id} has unusable memory state"
            )));
        };
        if !matches!(state.as_str(), "learning" | "review" | "relearning")
            || !stability.is_finite()
            || stability <= 0.0
            || stability > f32::MAX as f64
            || !difficulty.is_finite()
            || !(1.0..=10.0).contains(&difficulty)
        {
            return Err(AppError::BadRequest(format!(
                "Latest review for card {card_id} has unusable memory state"
            )));
        }
        sqlx::query(
            r#"
            INSERT INTO cards_states (card_id, user_id, stability, difficulty, last_review, state, review_revision)
            VALUES (?, ?, ?, ?, ?, ?, 1)
            ON CONFLICT(card_id, user_id) DO UPDATE SET
                stability = excluded.stability,
                difficulty = excluded.difficulty,
                last_review = excluded.last_review,
                state = excluded.state,
                review_revision = cards_states.review_revision + 1
            "#,
        )
        .bind(card_id)
        .bind(user_id)
        .bind(stability)
        .bind(difficulty)
        .bind(reviewed_at)
        .bind(state)
        .execute(&mut *tx)
        .await?;
        stats.card_states_derived += 1;
    }

    // Import suppressed cards
    for card_id in data.suppressed_cards {
        if !valid_card_ids.contains(&card_id) {
            warn!("Skipping suppression for non-existent card_id: {}", card_id);
            continue;
        }

        sqlx::query(
            r#"
            INSERT INTO users_card_flags (user_id, card_id, suppressed)
            VALUES (?, ?, 1)
            ON CONFLICT(user_id, card_id) DO UPDATE SET suppressed = 1
            "#,
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
        INSERT INTO users_settings (user_id, show_hanja_hints, red_threshold, yellow_threshold,
                                   day_boundary_hour, study_timezone, translation_language, auto_progress_on_correct, auto_progress_delay,
                                   desired_retention, daily_new_card_limit,
                                   history_colorized_area, history_colored_dots, history_threshold_lines)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
            show_hanja_hints = excluded.show_hanja_hints,
            red_threshold = excluded.red_threshold,
            yellow_threshold = excluded.yellow_threshold,
            day_boundary_hour = excluded.day_boundary_hour,
            study_timezone = COALESCE(excluded.study_timezone, users_settings.study_timezone),
            translation_language = COALESCE(excluded.translation_language, users_settings.translation_language),
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
    .bind(data.settings.core.show_hanja_hints)
    .bind(data.settings.core.red_threshold)
    .bind(data.settings.core.yellow_threshold)
    .bind(data.settings.core.day_boundary_hour)
    .bind(&data.settings.core.study_timezone)
    .bind(&data.settings.core.translation_language)
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
        let fsrs_params =
            serde_json::to_string(fsrs_params).map_err(|e| AppError::Internal(e.to_string()))?;
        sqlx::query(
            r#"
            INSERT INTO users_fsrs_parameters (user_id, parameters)
            VALUES (?, ?)
            ON CONFLICT(user_id) DO UPDATE SET parameters = excluded.parameters
            "#,
        )
        .bind(user_id)
        .bind(&fsrs_params)
        .execute(&mut *tx)
        .await?;
    }

    // Historical scheduled_days describes the policy at the time of review.
    // Current deadlines use the restored settings/model, including merge imports.
    crate::cards::scheduling::reschedule_user(&mut tx, user_id).await?;
    tx.commit().await?;

    info!("Import complete: {:?}", stats);

    Ok(Json(ImportDataResponse { stats }))
}

// Helper function to get user settings
async fn get_user_settings(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<UserSettingsExport, AppError> {
    crate::user::ensure_settings_row(pool, user_id).await?;

    let core = sqlx::query_as::<_, crate::user::UserSettingsCore>(
        r#"
        SELECT show_hanja_hints, red_threshold, yellow_threshold, day_boundary_hour, study_timezone, translation_language,
               auto_progress_on_correct, auto_progress_delay, desired_retention, daily_new_card_limit,
               history_colorized_area, history_colored_dots, history_threshold_lines
        FROM users_settings
        WHERE user_id = ?
        "#
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    // Get FSRS parameters if they exist
    let fsrs_parameters: Option<String> =
        sqlx::query_scalar("SELECT parameters FROM users_fsrs_parameters WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    let fsrs_parameters = fsrs_parameters
        .map(|json| serde_json::from_str(&json))
        .transpose()
        .map_err(|e| AppError::Internal(format!("Stored FSRS parameters are invalid: {e}")))?;

    Ok(UserSettingsExport {
        core,
        fsrs_parameters,
    })
}
#[cfg(test)]
#[path = "tests/export_import.rs"]
mod tests;
