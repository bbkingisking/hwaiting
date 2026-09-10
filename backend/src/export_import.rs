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
}

// UserSettingsCore (user.rs) is the `users_settings` row proper, flattened
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

// Export user data
#[utoipa::path(
    get,
    path = "/api/user/export",
    responses(
        (status = 200, description = "Full data export: settings, review history, suppressed cards", body = ExportData),
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
        FROM users_card_flags
        WHERE user_id = ? AND suppressed = 1
        "#
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

    info!("Export complete: {} reviews, {} suppressed cards",
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
        
        sqlx::query("DELETE FROM cards_states WHERE user_id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        
        sqlx::query("DELETE FROM users_card_flags WHERE user_id = ?")
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
    let valid_card_ids: std::collections::HashSet<i64> =
        sqlx::query_scalar("SELECT id FROM cards")
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .collect();

    // Import review history (must come before cards_states derivation)
    for review in data.review_history {
        if !valid_card_ids.contains(&review.card_id) {
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

    // Derive cards_states from the last review_history entry per card
    let derived_result = sqlx::query(
        r#"
        INSERT INTO cards_states (card_id, user_id, stability, difficulty, last_review, state)
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
        if !valid_card_ids.contains(&card_id) {
            warn!("Skipping suppression for non-existent card_id: {}", card_id);
            continue;
        }

        sqlx::query(
            r#"
            INSERT INTO users_card_flags (user_id, card_id, suppressed)
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
        INSERT INTO users_settings (user_id, show_percentage, red_threshold, yellow_threshold,
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
            INSERT INTO users_fsrs_parameters (user_id, parameters)
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
    // Ensure users_settings row exists
    sqlx::query(
        r#"
        INSERT INTO users_settings (user_id)
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
        FROM users_settings
        WHERE user_id = ?
        "#
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    // Get FSRS parameters if they exist
    let fsrs_parameters: Option<String> = sqlx::query_scalar(
        "SELECT parameters FROM users_fsrs_parameters WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(UserSettingsExport { core, fsrs_parameters })
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

        sqlx::query("INSERT INTO users_fsrs_parameters (user_id, parameters) VALUES (?, '[1.0,2.0,3.0]')")
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
    async fn export_then_import_round_trips_settings_history_and_suppressions() {
        let pool = test_pool().await;
        let source_id = test_user(&pool).await;
        setup_source_user(&pool, source_id).await;

        let exported = export_data(State(pool.clone()), AuthUser(source_id)).await.unwrap().0;
        assert_eq!(exported.review_history.len(), 3);
        assert_eq!(exported.suppressed_cards, vec![3]);

        let dest_id = test_user(&pool).await;
        let import_result = import_data(
            State(pool.clone()),
            AuthUser(dest_id),
            AppJson(ImportDataRequest { data: exported, overwrite: false }),
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
        let dest_settings = export_data(State(pool.clone()), AuthUser(dest_id)).await.unwrap().0.settings;
        assert_eq!(dest_settings.core.daily_new_card_limit, 7);
        assert_eq!(dest_settings.core.day_boundary_hour, 2);
        assert_eq!(dest_settings.fsrs_parameters.as_deref(), Some("[1.0,2.0,3.0]"));

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
    async fn overwrite_clears_existing_data_before_importing() {
        let pool = test_pool().await;
        let source_id = test_user(&pool).await;
        setup_source_user(&pool, source_id).await;
        let exported = export_data(State(pool.clone()), AuthUser(source_id)).await.unwrap().0;

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
        sqlx::query("INSERT INTO users_card_flags (user_id, card_id, suppressed) VALUES (?, 10, 1)")
            .bind(dest_id)
            .execute(&pool)
            .await
            .unwrap();

        let _ = import_data(
            State(pool.clone()),
            AuthUser(dest_id),
            AppJson(ImportDataRequest { data: exported, overwrite: true }),
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
        assert!(!has_old_review, "overwrite should have cleared the pre-existing review");

        let has_old_suppression: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM users_card_flags WHERE user_id = ? AND card_id = 10)",
        )
        .bind(dest_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!has_old_suppression, "overwrite should have cleared the pre-existing suppression");
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
                    show_percentage: true,
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
                    card_id: 1, // real
                    rating: "good".to_string(),
                    scheduled_days: None,
                    elapsed_days: None,
                    reviewed_at: "2026-01-01 00:00:00".to_string(),
                    stability: Some(1.0),
                    difficulty: Some(1.0),
                    state: Some("learning".to_string()),
                },
                ReviewHistoryExport {
                    card_id: 999999, // doesn't exist
                    rating: "good".to_string(),
                    scheduled_days: None,
                    elapsed_days: None,
                    reviewed_at: "2026-01-01 00:00:00".to_string(),
                    stability: Some(1.0),
                    difficulty: Some(1.0),
                    state: Some("learning".to_string()),
                },
            ],
            suppressed_cards: vec![2, 999999],
        };

        let result = import_data(
            State(pool.clone()),
            AuthUser(dest_id),
            AppJson(ImportDataRequest { data, overwrite: false }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(result.stats.reviews_imported, 1);
        assert_eq!(result.stats.suppressed_cards_imported, 1);
        assert_eq!(result.stats.card_states_derived, 1);
    }
}
