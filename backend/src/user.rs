use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool};
use tracing::info;
use utoipa::ToSchema;

use crate::error::{AppError, AppJson};
use crate::auth::AuthUser;

/// Lazily creates the `users_settings` row for `user_id` if it doesn't exist
/// yet - every settings read/write path needs this first, so it's shared
/// rather than each hand-repeating the same `INSERT ... ON CONFLICT DO
/// NOTHING`. Generic over the executor so `update_settings` can run it
/// inside its own transaction instead of a separate round trip against the
/// pool.
pub(crate) async fn ensure_settings_row<'e, E>(executor: E, user_id: i64) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    sqlx::query("INSERT INTO users_settings (user_id) VALUES (?) ON CONFLICT(user_id) DO NOTHING")
        .bind(user_id)
        .execute(executor)
        .await?;
    Ok(())
}

#[derive(Serialize, ToSchema)]
pub struct UserProfile {
    /// `None` for an account created via passkey, which has no username.
    pub username: Option<String>,
    /// The frontend has no other way to learn this once the password
    /// login form (and its localStorage-cached `is_admin`) is gone - it's
    /// read from here on app load instead.
    pub is_admin: bool,
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
#[derive(Serialize, Deserialize, sqlx::FromRow, ToSchema, Debug, PartialEq)]
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

    let (username, is_admin): (Option<String>, bool) = sqlx::query_as(
        "SELECT username, is_admin FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    Ok(Json(UserProfile { username, is_admin }))
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

    ensure_settings_row(&pool, user_id).await?;

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

    let UpdateSettingsRequest {
        show_percentage,
        red_threshold,
        yellow_threshold,
        day_boundary_hour,
        auto_progress_on_correct,
        auto_progress_delay,
        desired_retention,
        daily_new_card_limit,
        history_colorized_area,
        history_colored_dots,
        history_threshold_lines,
    } = payload;

    // Validate before writing anything, same bounds as before.
    if let Some(v) = day_boundary_hour && !(0..=23).contains(&v) {
        return Err(AppError::BadRequest("day_boundary_hour must be between 0 and 23".to_string()));
    }
    if let Some(v) = auto_progress_delay && !(0..=3000).contains(&v) {
        return Err(AppError::BadRequest("auto_progress_delay must be between 0 and 3000".to_string()));
    }
    if let Some(v) = desired_retention && !(0.5..=0.99).contains(&v) {
        return Err(AppError::BadRequest("desired_retention must be between 0.5 and 0.99".to_string()));
    }
    if let Some(v) = daily_new_card_limit && v.is_negative() {
        return Err(AppError::BadRequest("new daily card limit must be a positive integer".to_string()));
    }

    // One transaction for the lazy row creation and the update itself, and
    // one dynamically-built UPDATE for every provided field - same pattern
    // as admin::edit_card's cards/cards_translations updates - rather than
    // up to 11 separate unguarded statements against the pool. Atomicity
    // matters here specifically because there used to be none: a failure
    // partway through the old field-by-field version could leave settings
    // half-applied.
    let mut tx = pool.begin().await?;
    ensure_settings_row(&mut *tx, user_id).await?;

    let mut sets: Vec<&str> = Vec::new();
    if show_percentage.is_some()       { sets.push("show_percentage = ?") }
    if red_threshold.is_some()         { sets.push("red_threshold = ?") }
    if yellow_threshold.is_some()      { sets.push("yellow_threshold = ?") }
    if day_boundary_hour.is_some()     { sets.push("day_boundary_hour = ?") }
    if auto_progress_on_correct.is_some() { sets.push("auto_progress_on_correct = ?") }
    if auto_progress_delay.is_some()   { sets.push("auto_progress_delay = ?") }
    if desired_retention.is_some()     { sets.push("desired_retention = ?") }
    if daily_new_card_limit.is_some()  { sets.push("daily_new_card_limit = ?") }
    if history_colorized_area.is_some()   { sets.push("history_colorized_area = ?") }
    if history_colored_dots.is_some()     { sets.push("history_colored_dots = ?") }
    if history_threshold_lines.is_some()  { sets.push("history_threshold_lines = ?") }

    if !sets.is_empty() {
        let sql = format!("UPDATE users_settings SET {} WHERE user_id = ?", sets.join(", "));
        let mut q = sqlx::query(&sql);
        if let Some(v) = show_percentage       { q = q.bind(v) }
        if let Some(v) = red_threshold         { q = q.bind(v) }
        if let Some(v) = yellow_threshold      { q = q.bind(v) }
        if let Some(v) = day_boundary_hour     { q = q.bind(v) }
        if let Some(v) = auto_progress_on_correct { q = q.bind(v) }
        if let Some(v) = auto_progress_delay   { q = q.bind(v) }
        if let Some(v) = desired_retention     { q = q.bind(v) }
        if let Some(v) = daily_new_card_limit  { q = q.bind(v) }
        if let Some(v) = history_colorized_area   { q = q.bind(v) }
        if let Some(v) = history_colored_dots     { q = q.bind(v) }
        if let Some(v) = history_threshold_lines  { q = q.bind(v) }
        q.bind(user_id).execute(&mut *tx).await?;
    }

    tx.commit().await?;

    info!("Settings updated successfully");

    Ok(Json(UpdateSettingsResponse { success: true }))
}
