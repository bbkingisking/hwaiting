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

    // Import review history (must come before cards_states derivation)
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