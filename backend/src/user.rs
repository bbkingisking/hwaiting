use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::info;
use utoipa::ToSchema;

use crate::error::{AppError, AppJson};
use crate::auth::AuthUser;

#[derive(Serialize, ToSchema)]
pub struct UserProfile {
    pub username: String,
}

/// The `users_settings` row proper - every field shared verbatim between the
/// live API response (`UserSettings`, below) and data export/import
/// (`export_import::UserSettingsExport`). Split out so both flatten this
/// struct instead of hand-declaring the same 11 fields a second time: the
/// export path used to be its own independently-declared struct that merely
/// happened to agree with `UserSettings`, the same silent-drift risk
/// `cards::Card` used to carry before it was unified from
/// `CardFront`/`CardBack` (see its doc comment in cards/mod.rs) - a settings
/// field added to the DB and to one of these two but not the other would
/// silently vanish from export, or from the live API, with no compiler
/// error either way. `sqlx::FromRow` lets both `get_settings` (below) and
/// `export_import::get_user_settings` read a row straight into this shape
/// too, rather than each hand-repeating the same 11 `row.get(...)` calls.
#[derive(Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct UserSettingsCore {
    pub show_percentage: bool,
    pub red_threshold: i64,
    pub yellow_threshold: i64,
    pub day_boundary_hour: i64,
    pub auto_progress_on_correct: bool,
    pub auto_progress_delay: i64,
    pub desired_retention: f64,
    pub daily_new_card_limit: i64,
    pub history_colorized_area: bool,
    pub history_colored_dots: bool,
    pub history_threshold_lines: bool,
}

#[derive(Serialize, ToSchema)]
pub struct UserSettings {
    #[serde(flatten)]
    pub core: UserSettingsCore,
    /// Whether the user has FSRS parameters fitted from their own review
    /// history, as opposed to library defaults - a presence flag, not the
    /// parameters themselves. See `UserSettingsExport::fsrs_parameters` for
    /// the portable form export/import actually needs; the two aren't the
    /// same field under different names, so this doesn't join `core`.
    pub has_fsrs_parameters: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateSettingsRequest {
    pub show_percentage: Option<bool>,
    pub red_threshold: Option<i64>,
    pub yellow_threshold: Option<i64>,
    pub day_boundary_hour: Option<i64>,
    pub auto_progress_on_correct: Option<bool>,
    pub auto_progress_delay: Option<i64>,
    pub desired_retention: Option<f64>,
    pub daily_new_card_limit: Option<i64>,
    pub history_colorized_area: Option<bool>,
    pub history_colored_dots: Option<bool>,
    pub history_threshold_lines: Option<bool>,
}



#[derive(Serialize, ToSchema)]
pub struct UpdateSettingsResponse {
    pub success: bool,
}

// Get current user's profile
#[utoipa::path(
    get,
    path = "/api/user/me",
    responses(
        (status = 200, description = "Current user's profile", body = UserProfile),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "user"
)]
pub async fn get_profile(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<UserProfile>, AppError> {
    let user_id = auth.0;
    info!("Getting profile for user_id: {}", user_id);

    let username: String = sqlx::query_scalar(
        "SELECT username FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    Ok(Json(UserProfile { username }))
}

// Get user settings
#[utoipa::path(
    get,
    path = "/api/user/settings",
    responses(
        (status = 200, description = "Current user settings (row lazily created on first access)", body = UserSettings),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "user"
)]
pub async fn get_settings(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<UserSettings>, AppError> {
    let user_id = auth.0;
    info!("Getting settings for user_id: {}", user_id);

    // Ensure users_settings row exists
    sqlx::query(
        r#"
        INSERT INTO users_settings (user_id)
        VALUES (?)
        ON CONFLICT(user_id) DO NOTHING
        "#
    )
    .bind(user_id)
    .execute(&pool)
    .await?;

    let core = sqlx::query_as::<_, UserSettingsCore>(
        r#"
        SELECT show_percentage, red_threshold, yellow_threshold, day_boundary_hour, auto_progress_on_correct, auto_progress_delay, desired_retention, daily_new_card_limit, history_colorized_area, history_colored_dots, history_threshold_lines
        FROM users_settings
        WHERE user_id = ?
        "#
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    // Check if user has custom FSRS parameters
    let has_fsrs_parameters: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users_fsrs_parameters WHERE user_id = ?)"
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    Ok(Json(UserSettings { core, has_fsrs_parameters }))
}

// Update user settings
#[utoipa::path(
    patch,
    path = "/api/user/settings",
    request_body = UpdateSettingsRequest,
    responses(
        (status = 200, description = "Settings updated (partial update, only provided fields written)", body = UpdateSettingsResponse),
        (status = 400, description = "Out-of-range value or malformed request", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "user"
)]
pub async fn update_settings(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    AppJson(payload): AppJson<UpdateSettingsRequest>,
) -> Result<Json<UpdateSettingsResponse>, AppError> {
    let user_id = auth.0;
    info!("Updating settings for user_id: {}", user_id);

    // Ensure users_settings row exists
    sqlx::query(
        r#"
        INSERT INTO users_settings (user_id)
        VALUES (?)
        ON CONFLICT(user_id) DO NOTHING
        "#
    )
    .bind(user_id)
    .execute(&pool)
    .await?;

    // Update individual fields if provided
    if let Some(show_percentage) = payload.show_percentage {
        sqlx::query("UPDATE users_settings SET show_percentage = ? WHERE user_id = ?")
            .bind(show_percentage)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(red_threshold) = payload.red_threshold {
        sqlx::query("UPDATE users_settings SET red_threshold = ? WHERE user_id = ?")
            .bind(red_threshold)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(yellow_threshold) = payload.yellow_threshold {
        sqlx::query("UPDATE users_settings SET yellow_threshold = ? WHERE user_id = ?")
            .bind(yellow_threshold)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(day_boundary_hour) = payload.day_boundary_hour {
        // Validate hour is between 0 and 23
        if day_boundary_hour < 0 || day_boundary_hour > 23 {
            return Err(AppError::BadRequest("day_boundary_hour must be between 0 and 23".to_string()));
        }
        sqlx::query("UPDATE users_settings SET day_boundary_hour = ? WHERE user_id = ?")
            .bind(day_boundary_hour)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(auto_progress_on_correct) = payload.auto_progress_on_correct {
        sqlx::query("UPDATE users_settings SET auto_progress_on_correct = ? WHERE user_id = ?")
            .bind(auto_progress_on_correct)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(auto_progress_delay) = payload.auto_progress_delay {
        if auto_progress_delay < 0 || auto_progress_delay > 3000 {
            return Err(AppError::BadRequest("auto_progress_delay must be between 0 and 3000".to_string()));
        }
        sqlx::query("UPDATE users_settings SET auto_progress_delay = ? WHERE user_id = ?")
            .bind(auto_progress_delay)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(desired_retention) = payload.desired_retention {
        if desired_retention < 0.5 || desired_retention > 0.99 {
            return Err(AppError::BadRequest("desired_retention must be between 0.5 and 0.99".to_string()));
        }
        sqlx::query("UPDATE users_settings SET desired_retention = ? WHERE user_id = ?")
            .bind(desired_retention)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(daily_new_card_limit) = payload.daily_new_card_limit {
        if daily_new_card_limit.is_negative() {
            return Err(AppError::BadRequest("new daily card limit must be a positive integer".to_string()))
        }
        sqlx::query("UPDATE users_settings SET daily_new_card_limit = ? WHERE user_id = ?")
            .bind(daily_new_card_limit)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(v) = payload.history_colorized_area {
        sqlx::query("UPDATE users_settings SET history_colorized_area = ? WHERE user_id = ?")
            .bind(v)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(v) = payload.history_colored_dots {
        sqlx::query("UPDATE users_settings SET history_colored_dots = ? WHERE user_id = ?")
            .bind(v)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(v) = payload.history_threshold_lines {
        sqlx::query("UPDATE users_settings SET history_threshold_lines = ? WHERE user_id = ?")
            .bind(v)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    info!("Settings updated successfully");

    Ok(Json(UpdateSettingsResponse { success: true }))
}
