use axum::{Json, extract::State};
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::{info, warn};
use utoipa::ToSchema;

use crate::auth::AuthUser;
use crate::error::{AppError, AppJson};

// Export/Import data structures

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ExportData {
    pub version: String,
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
    pub fsrs_parameters: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ReviewHistoryExport {
    pub card_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentence_id: Option<i64>,
    pub rating: String,
    pub scheduled_days: Option<f64>,
    pub elapsed_days: Option<f64>,
    pub reviewed_at: String,
    pub stability: Option<f64>,
    pub difficulty: Option<f64>,
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
        Some(data.settings.core.day_boundary_hour),
        Some(data.settings.core.auto_progress_delay),
        Some(data.settings.core.desired_retention),
        Some(data.settings.core.daily_new_card_limit),
    )?;
    // Construct the actual model; parsing the JSON alone does not establish
    // that its parameter count and values can schedule a card.
    crate::cards::scheduling::Scheduler::new(
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
        .map(|row| ReviewHistoryExport {
            card_id: row.get("card_id"),
            sentence_id: row.get("sentence_id"),
            rating: row.get("rating"),
            scheduled_days: row.get("scheduled_days"),
            elapsed_days: row.get("elapsed_days"),
            reviewed_at: row.get("reviewed_at"),
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
        .collect();

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
        exported_at: chrono::Utc::now().to_rfc3339(),
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

    // Begin transaction
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    // If overwrite, delete existing data
    if payload.overwrite {
        info!("Overwrite enabled - clearing existing data");

        // SQLite foreign-key enforcement is disabled for this pool, so clear
        // dependent mistake rows explicitly before deleting their reviews.
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

        // Keep revision tombstones so an old prompt cannot become valid again
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
        let sentence_id =
            proposed_sentence.filter(|id| valid_target_cards.get(id) == Some(&review.card_id));
        if proposed_sentence.is_some() && sentence_id.is_none() {
            warn!(
                "Skipping invalid sentence_id on imported review for card_id: {}",
                review.card_id
            );
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
        .bind(review.reviewed_at)
        .bind(review.stability)
        .bind(review.difficulty)
        .bind(review.state)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();

        if let Some(wrong_answer) = review.wrong_answer {
            if valid_target_cards.get(&wrong_answer.target_sentence_id) == Some(&review.card_id)
                && sentence_id == Some(wrong_answer.target_sentence_id)
            {
                sqlx::query(
                    "INSERT INTO wrong_answers (review_id, target_sentence_id, answer)
                     VALUES (?, ?, ?)",
                )
                .bind(review_id)
                .bind(wrong_answer.target_sentence_id)
                .bind(wrong_answer.answer)
                .execute(&mut *tx)
                .await?;
            } else {
                warn!(
                    "Skipping wrong answer for missing or mismatched target_sentence_id: {}",
                    wrong_answer.target_sentence_id
                );
            }
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
                                   day_boundary_hour, auto_progress_on_correct, auto_progress_delay,
                                   desired_retention, daily_new_card_limit,
                                   history_colorized_area, history_colored_dots, history_threshold_lines)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
            show_hanja_hints = excluded.show_hanja_hints,
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
    .bind(data.settings.core.show_hanja_hints)
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
            INSERT INTO users_fsrs_parameters (user_id, parameters)
            VALUES (?, ?)
            ON CONFLICT(user_id) DO UPDATE SET parameters = excluded.parameters
            "#,
        )
        .bind(user_id)
        .bind(fsrs_params)
        .execute(&mut *tx)
        .await?;
    }

    // Historical scheduled_days describes the policy at the time of review.
    // Current deadlines use the restored settings/model, including merge imports.
    crate::cards::scheduling::reschedule_user(&mut tx, user_id).await?;
    tx.commit().await?;

    info!("Import complete: {:?}", stats);

    Ok(Json(ImportDataResponse {
        success: true,
        message: "Data imported successfully".to_string(),
        stats,
    }))
}

// Helper function to get user settings
async fn get_user_settings(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<UserSettingsExport, AppError> {
    crate::user::ensure_settings_row(pool, user_id).await?;

    let core = sqlx::query_as::<_, crate::user::UserSettingsCore>(
        r#"
        SELECT show_hanja_hints, red_threshold, yellow_threshold, day_boundary_hour,
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

    Ok(UserSettingsExport {
        core,
        fsrs_parameters,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_pool, test_user};

    async fn setup_source_user(pool: &SqlitePool, user_id: i64) {
        sqlx::query(
            "INSERT INTO users_settings (user_id, daily_new_card_limit, day_boundary_hour) VALUES (?, 7, 2)",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO users_fsrs_parameters (user_id, parameters) VALUES (?, '[0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2]')",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();

        // Two reviews of card 1 (card 1 ends up "review"), one of card 2.
        sqlx::query(
            "INSERT INTO review_history (user_id, card_id, rating, reviewed_at, stability, difficulty, state) \
             VALUES (?, 1, 'good', '2026-01-01 00:00:00', 1, 1, 'learning')",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO review_history (user_id, card_id, rating, reviewed_at, stability, difficulty, state) \
             VALUES (?, 1, 'good', '2026-01-04 00:00:00', 3, 2, 'review')",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO review_history (user_id, card_id, rating, reviewed_at, stability, difficulty, state) \
             VALUES (?, 2, 'again', '2026-01-02 00:00:00', 1, 1, 'learning')",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();

        sqlx::query("INSERT INTO users_card_flags (user_id, card_id, suppressed) VALUES (?, 3, 1)")
            .bind(user_id)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn invalid_settings_match_patch_and_preserve_existing_data() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        setup_source_user(&pool, user_id).await;
        sqlx::query("INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, review_revision) VALUES (?, 1, 3, 2, '2026-01-04', 'review', 7)")
            .bind(user_id).execute(&pool).await.unwrap();

        let before = serde_json::to_value(
            export_data(State(pool.clone()), AuthUser(user_id))
                .await
                .unwrap()
                .0,
        )
        .unwrap();
        for (field, value) in [
            ("day_boundary_hour", serde_json::json!(-1)),
            ("day_boundary_hour", serde_json::json!(24)),
            ("auto_progress_delay", serde_json::json!(-1)),
            ("auto_progress_delay", serde_json::json!(3001)),
            ("desired_retention", serde_json::json!(0.49)),
            ("desired_retention", serde_json::json!(1.0)),
            ("daily_new_card_limit", serde_json::json!(-1)),
        ] {
            let patch: crate::user::UpdateSettingsRequest =
                serde_json::from_value(serde_json::json!({field: value})).unwrap();
            let patch_error = match crate::user::update_settings(
                State(pool.clone()),
                AuthUser(user_id),
                AppJson(patch),
            )
            .await
            {
                Err(AppError::BadRequest(message)) => message,
                _ => panic!("PATCH should reject {field}={value}"),
            };
            for overwrite in [false, true] {
                let mut invalid = before.clone();
                invalid["settings"][field] = value.clone();
                // A successful overwrite would remove the existing history/flags.
                invalid["review_history"] = serde_json::json!([]);
                invalid["suppressed_cards"] = serde_json::json!([]);
                match import_data(
                    State(pool.clone()),
                    AuthUser(user_id),
                    AppJson(ImportDataRequest {
                        data: serde_json::from_value(invalid).unwrap(),
                        overwrite,
                    }),
                )
                .await
                {
                    Err(AppError::BadRequest(message)) => assert_eq!(message, patch_error),
                    _ => panic!("import should reject {field}={value}"),
                }
                let mut after = serde_json::to_value(
                    export_data(State(pool.clone()), AuthUser(user_id))
                        .await
                        .unwrap()
                        .0,
                )
                .unwrap();
                after["exported_at"] = before["exported_at"].clone();
                assert_eq!(after, before, "invalid import must preserve existing data");
                let state: (f64, f64, String, String, i64) = sqlx::query_as(
                    "SELECT stability, difficulty, last_review, state, review_revision FROM cards_states WHERE user_id = ? AND card_id = 1",
                ).bind(user_id).fetch_one(&pool).await.unwrap();
                assert_eq!(state, (3.0, 2.0, "2026-01-04".into(), "review".into(), 7));
            }
        }
    }

    #[tokio::test]
    async fn invalid_backup_cannot_replace_existing_history_or_state() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        setup_source_user(&pool, user_id).await;
        sqlx::query("INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, review_revision) VALUES (?, 1, 3, 2, '2026-01-04', 'review', 7)")
            .bind(user_id).execute(&pool).await.unwrap();

        let before = serde_json::to_value(
            export_data(State(pool.clone()), AuthUser(user_id))
                .await
                .unwrap()
                .0,
        )
        .unwrap();
        let state_before: (f64, f64, String, String, i64) = sqlx::query_as(
            "SELECT stability, difficulty, last_review, state, review_revision FROM cards_states WHERE user_id = ? AND card_id = 1",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        for (name, path, invalid) in [
            (
                "FSRS shape",
                "fsrs_parameters",
                serde_json::json!("[1,2,3]"),
            ),
            (
                "export timestamp",
                "exported_at",
                serde_json::json!("not a date"),
            ),
            (
                "review timestamp",
                "reviewed_at",
                serde_json::json!("not a date"),
            ),
            ("rating", "rating", serde_json::json!("perfect")),
            ("scheduled days", "scheduled_days", serde_json::json!(-1)),
            ("elapsed days", "elapsed_days", serde_json::json!(-1)),
            ("zero stability", "stability", serde_json::json!(0)),
            (
                "unrepresentable stability",
                "stability",
                serde_json::json!(1e40),
            ),
            ("difficulty", "difficulty", serde_json::json!(11)),
            ("missing difficulty", "difficulty", serde_json::Value::Null),
            ("invalid state", "state", serde_json::json!("new")),
        ] {
            let mut invalid_data = before.clone();
            match path {
                "fsrs_parameters" => invalid_data["settings"][path] = invalid,
                "exported_at" => invalid_data[path] = invalid,
                _ => invalid_data["review_history"][0][path] = invalid,
            }
            let result = import_data(
                State(pool.clone()),
                AuthUser(user_id),
                AppJson(ImportDataRequest {
                    data: serde_json::from_value(invalid_data).unwrap(),
                    overwrite: true,
                }),
            )
            .await;
            assert!(matches!(result, Err(AppError::BadRequest(_))), "{name}");
            let mut after = serde_json::to_value(
                export_data(State(pool.clone()), AuthUser(user_id))
                    .await
                    .unwrap()
                    .0,
            )
            .unwrap();
            after["exported_at"] = before["exported_at"].clone();
            assert_eq!(after, before, "{name} must preserve existing data");
            let state_after: (f64, f64, String, String, i64) = sqlx::query_as(
                "SELECT stability, difficulty, last_review, state, review_revision FROM cards_states WHERE user_id = ? AND card_id = 1",
            )
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(state_after, state_before, "{name} must preserve card state");
        }

        // A historical row may lack memory state, but the final review of a
        // restored card must provide it. This is checked inside the transaction.
        let mut invalid_data = before.clone();
        invalid_data["review_history"][1]["stability"] = serde_json::Value::Null;
        invalid_data["review_history"][1]["difficulty"] = serde_json::Value::Null;
        invalid_data["review_history"][1]["state"] = serde_json::Value::Null;
        let result = import_data(
            State(pool.clone()),
            AuthUser(user_id),
            AppJson(ImportDataRequest {
                data: serde_json::from_value(invalid_data).unwrap(),
                overwrite: true,
            }),
        )
        .await;
        assert!(matches!(result, Err(AppError::BadRequest(_))));
        let mut after = serde_json::to_value(
            export_data(State(pool.clone()), AuthUser(user_id))
                .await
                .unwrap()
                .0,
        )
        .unwrap();
        after["exported_at"] = before["exported_at"].clone();
        assert_eq!(after, before);
        let state_after: (f64, f64, String, String, i64) = sqlx::query_as(
            "SELECT stability, difficulty, last_review, state, review_revision FROM cards_states WHERE user_id = ? AND card_id = 1",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state_after, state_before);
    }

    #[tokio::test]
    async fn import_uses_latest_instant_with_mixed_timestamp_formats() {
        let pool = test_pool().await;
        let source_id = test_user(&pool).await;
        setup_source_user(&pool, source_id).await;
        let mut data = export_data(State(pool.clone()), AuthUser(source_id))
            .await
            .unwrap()
            .0;
        // The later instant sorts before the older one as text because the
        // older timestamp has a 'T' where the later one has a space.
        data.review_history[0].reviewed_at = "2026-01-03T00:00:00Z".into();
        data.review_history[0].stability = Some(1.0);
        data.review_history[2].reviewed_at = "2026-01-04 00:00:00".into();
        data.review_history[2].stability = Some(4.0);
        let dest_id = test_user(&pool).await;
        let result = import_data(
            State(pool.clone()),
            AuthUser(dest_id),
            AppJson(ImportDataRequest {
                data,
                overwrite: false,
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(result.stats.card_states_derived, 2);
        let (stability, last_review): (f64, String) = sqlx::query_as(
            "SELECT stability, last_review FROM cards_states WHERE user_id = ? AND card_id = 1",
        )
        .bind(dest_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stability, 4.0);
        assert_eq!(last_review, "2026-01-04 00:00:00");
    }

    #[tokio::test]
    async fn imports_accept_settings_at_validation_boundaries() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        for (hour, delay, retention, limit) in [(0, 0, 0.5, 0), (23, 3000, 0.99, i64::MAX)] {
            let mut data = export_data(State(pool.clone()), AuthUser(user_id))
                .await
                .unwrap()
                .0;
            data.settings.core.day_boundary_hour = hour;
            data.settings.core.auto_progress_delay = delay;
            data.settings.core.desired_retention = retention;
            data.settings.core.daily_new_card_limit = limit;
            let _ = import_data(
                State(pool.clone()),
                AuthUser(user_id),
                AppJson(ImportDataRequest {
                    data,
                    overwrite: true,
                }),
            )
            .await
            .unwrap();
            let settings = get_user_settings(&pool, user_id).await.unwrap().core;
            assert_eq!(settings.day_boundary_hour, hour);
            assert_eq!(settings.auto_progress_delay, delay);
            assert_eq!(settings.desired_retention, retention);
            assert_eq!(settings.daily_new_card_limit, limit);
        }
    }

    #[tokio::test]
    async fn export_then_import_round_trips_settings_history_and_suppressions() {
        let pool = test_pool().await;
        let source_id = test_user(&pool).await;
        setup_source_user(&pool, source_id).await;

        sqlx::query("UPDATE users_settings SET show_hanja_hints = 0 WHERE user_id = ?")
            .bind(source_id)
            .execute(&pool)
            .await
            .unwrap();

        let exported = export_data(State(pool.clone()), AuthUser(source_id))
            .await
            .unwrap()
            .0;
        assert!(!exported.settings.core.show_hanja_hints);
        assert_eq!(exported.review_history.len(), 3);
        assert_eq!(exported.suppressed_cards, vec![3]);

        let dest_id = test_user(&pool).await;
        let import_result = import_data(
            State(pool.clone()),
            AuthUser(dest_id),
            AppJson(ImportDataRequest {
                data: exported,
                overwrite: false,
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(import_result.stats.reviews_imported, 3);
        assert_eq!(import_result.stats.suppressed_cards_imported, 1);
        // One derived cards_states row per distinct card_id in the imported
        // history (cards 1 and 2).
        assert_eq!(import_result.stats.card_states_derived, 2);

        // Settings round-tripped, including the FSRS parameters.
        let dest_settings = export_data(State(pool.clone()), AuthUser(dest_id))
            .await
            .unwrap()
            .0
            .settings;
        assert_eq!(dest_settings.core.daily_new_card_limit, 7);
        assert_eq!(dest_settings.core.day_boundary_hour, 2);
        assert!(!dest_settings.core.show_hanja_hints);
        assert_eq!(
            dest_settings.fsrs_parameters.as_deref(),
            Some(
                "[0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2]"
            )
        );

        // cards_states derived from the *latest* review per card: card 1's
        // last review (1/4) had stability 3, not the first review's 1.
        let (stability, state): (f64, String) = sqlx::query_as(
            "SELECT stability, state FROM cards_states WHERE user_id = ? AND card_id = 1",
        )
        .bind(dest_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stability, 3.0);
        assert_eq!(state, "review");

        let suppressed: Vec<i64> = sqlx::query_scalar(
            "SELECT card_id FROM users_card_flags WHERE user_id = ? AND suppressed = 1",
        )
        .bind(dest_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(suppressed, vec![3]);
    }

    #[tokio::test]
    async fn import_fsrs_parameters_respects_overwrite_and_backup_defaults() {
        let pool = test_pool().await;
        let source_id = test_user(&pool).await;
        setup_source_user(&pool, source_id).await;

        for overwrite in [false, true] {
            for replacement in [
                None,
                Some(
                    "[0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3]",
                ),
            ] {
                let dest_id = test_user(&pool).await;
                sqlx::query("INSERT INTO users_fsrs_parameters (user_id, parameters) VALUES (?, '[0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4]')")
                    .bind(dest_id).execute(&pool).await.unwrap();
                let mut data = export_data(State(pool.clone()), AuthUser(source_id))
                    .await
                    .unwrap()
                    .0;
                data.settings.fsrs_parameters = replacement.map(str::to_owned);

                let _ = import_data(
                    State(pool.clone()),
                    AuthUser(dest_id),
                    AppJson(ImportDataRequest { data, overwrite }),
                )
                .await
                .unwrap();

                let settings = get_user_settings(&pool, dest_id).await.unwrap();
                let expected = replacement.or({
                    if overwrite {
                        None
                    } else {
                        Some("[0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4]")
                    }
                });
                assert_eq!(settings.fsrs_parameters.as_deref(), expected);
                // Clearing the destination must leave other users' parameters intact.
                let source = get_user_settings(&pool, source_id).await.unwrap();
                assert_eq!(
                    source.fsrs_parameters.as_deref(),
                    Some(
                        "[0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2]"
                    )
                );
            }
        }
    }

    #[tokio::test]
    async fn overwrite_clears_existing_data_before_importing() {
        let pool = test_pool().await;
        let source_id = test_user(&pool).await;
        setup_source_user(&pool, source_id).await;
        let exported = export_data(State(pool.clone()), AuthUser(source_id))
            .await
            .unwrap()
            .0;

        let dest_id = test_user(&pool).await;
        // Pre-existing data that overwrite:true should wipe.
        sqlx::query(
            "INSERT INTO review_history (user_id, card_id, rating, reviewed_at, state) \
             VALUES (?, 10, 'easy', '2020-01-01 00:00:00', 'review')",
        )
        .bind(dest_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO users_card_flags (user_id, card_id, suppressed) VALUES (?, 10, 1)",
        )
        .bind(dest_id)
        .execute(&pool)
        .await
        .unwrap();

        let _ = import_data(
            State(pool.clone()),
            AuthUser(dest_id),
            AppJson(ImportDataRequest {
                data: exported,
                overwrite: true,
            }),
        )
        .await
        .unwrap();

        let has_old_review: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM review_history WHERE user_id = ? AND card_id = 10)",
        )
        .bind(dest_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            !has_old_review,
            "overwrite should have cleared the pre-existing review"
        );

        let has_old_suppression: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM users_card_flags WHERE user_id = ? AND card_id = 10)",
        )
        .bind(dest_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            !has_old_suppression,
            "overwrite should have cleared the pre-existing suppression"
        );
    }

    #[tokio::test]
    async fn unknown_card_ids_are_skipped_and_counted_correctly() {
        let pool = test_pool().await;
        let dest_id = test_user(&pool).await;

        let data = ExportData {
            version: "1.0".to_string(),
            exported_at: chrono::Utc::now().to_rfc3339(),
            settings: UserSettingsExport {
                core: crate::user::UserSettingsCore {
                    show_hanja_hints: true,
                    red_threshold: 50,
                    yellow_threshold: 70,
                    day_boundary_hour: 4,
                    auto_progress_on_correct: false,
                    auto_progress_delay: 1500,
                    desired_retention: 0.9,
                    daily_new_card_limit: 20,
                    history_colorized_area: false,
                    history_colored_dots: false,
                    history_threshold_lines: false,
                },
                fsrs_parameters: None,
            },
            review_history: vec![
                ReviewHistoryExport {
                    sentence_id: None,
                    card_id: 1, // real
                    rating: "good".to_string(),
                    scheduled_days: None,
                    elapsed_days: None,
                    reviewed_at: "2026-01-01 00:00:00".to_string(),
                    stability: Some(1.0),
                    difficulty: Some(1.0),
                    state: Some("learning".to_string()),
                    wrong_answer: None,
                },
                ReviewHistoryExport {
                    sentence_id: None,
                    card_id: 999999, // doesn't exist
                    rating: "good".to_string(),
                    scheduled_days: None,
                    elapsed_days: None,
                    reviewed_at: "2026-01-01 00:00:00".to_string(),
                    stability: Some(1.0),
                    difficulty: Some(1.0),
                    state: Some("learning".to_string()),
                    wrong_answer: None,
                },
            ],
            suppressed_cards: vec![2, 999999],
        };

        let result = import_data(
            State(pool.clone()),
            AuthUser(dest_id),
            AppJson(ImportDataRequest {
                data,
                overwrite: false,
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(result.stats.reviews_imported, 1);
        assert_eq!(result.stats.suppressed_cards_imported, 1);
        assert_eq!(result.stats.card_states_derived, 1);
        let imported_sentence: Option<i64> = sqlx::query_scalar(
            "SELECT sentence_id FROM review_history WHERE user_id = ? AND card_id = 1",
        )
        .bind(dest_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(imported_sentence, Some(1));
    }

    #[tokio::test]
    async fn imports_advance_revisions_and_overwrite_preserves_reset_markers() {
        let pool = test_pool().await;
        let source_id = test_user(&pool).await;
        setup_source_user(&pool, source_id).await;
        let dest_id = test_user(&pool).await;
        sqlx::query("INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, review_revision) VALUES (?, 10, 3, 2, '2026-01-01 00:00:00', 'review', 7)")
            .bind(dest_id).execute(&pool).await.unwrap();
        // Card 1 initially has no row (revision zero). Merging must invalidate
        // that prompt; later merges and overwrites must keep advancing it.
        let mut previous_revision = 0;
        for overwrite in [false, false, true, true] {
            let data = export_data(State(pool.clone()), AuthUser(source_id))
                .await
                .unwrap()
                .0;
            let _ = import_data(
                State(pool.clone()),
                AuthUser(dest_id),
                AppJson(ImportDataRequest { data, overwrite }),
            )
            .await
            .unwrap();
            let revision: i64 = sqlx::query_scalar(
                "SELECT review_revision FROM cards_states WHERE user_id = ? AND card_id = 1",
            )
            .bind(dest_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert!(revision > previous_revision);
            previous_revision = revision;
        }
        // Card 10 is absent from the import: its learning history is cleared,
        // but its revision must survive so a prompt with revision 7 stays stale.
        let (revision, last_review, stability, state): (i64, Option<String>, f64, String) = sqlx::query_as(
            "SELECT review_revision, last_review, stability, state FROM cards_states WHERE user_id = ? AND card_id = 10")
            .bind(dest_id).fetch_one(&pool).await.unwrap();
        assert_eq!(revision, 9);
        assert_eq!(last_review, None);
        assert_eq!(stability, 0.0);
        assert_eq!(state, "new");
        let stale = crate::cards::check_answer(
            State(pool.clone()),
            crate::error::AppPath(10),
            AuthUser(dest_id),
            AppJson(crate::cards::CheckRequest {
                answer: "test".into(),
                sentence_id: 10,
                review_revision: 7,
                content_revision: 0,
            }),
        )
        .await;
        assert!(matches!(stale, Err(AppError::StaleReview)));
    }
}
