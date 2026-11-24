use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool};
use tracing::info;
use utoipa::ToSchema;

use crate::auth::AuthUser;
use crate::error::{AppError, AppJson};

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

/// UTC is the stable fallback for API-only accounts awaiting initialization.
pub(crate) fn parse_study_timezone(name: Option<&str>) -> Result<chrono_tz::Tz, AppError> {
    name.unwrap_or("UTC")
        .parse()
        .map_err(|_| AppError::BadRequest("study_timezone must be a valid IANA timezone".into()))
}

#[derive(Serialize, ToSchema)]
pub struct UserProfile {
    /// Null for a passkey-only account without a username.
    #[schema(required)]
    pub username: Option<String>,
    /// Whether this account can use administrator endpoints.
    pub is_admin: bool,
}

/// Study preferences shared by live settings and portable backups.
#[derive(Serialize, Deserialize, sqlx::FromRow, ToSchema, Debug, PartialEq)]
pub struct UserSettingsCore {
    /// Controls Hanja hint visibility on review cards.
    #[schema(default = true)]
    pub show_hanja_hints: bool,
    /// History accuracy percentage below which days are shown red; below `yellow_threshold`.
    #[schema(minimum = 0, maximum = 100)]
    pub red_threshold: i64,
    /// History accuracy percentage below which days are shown yellow; above `red_threshold`.
    #[schema(minimum = 0, maximum = 100)]
    pub yellow_threshold: i64,
    #[schema(minimum = 0, maximum = 23)]
    pub day_boundary_hour: i64,
    /// Saved IANA study timezone (e.g. `Asia/Seoul`); null until a new account
    /// initializes it. In a backup import, null keeps the saved timezone.
    // `deserialize_with` disables serde's implicit None for a missing Option,
    // so a backup must state the key (null is fine), matching the schema.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schema(required)]
    pub study_timezone: Option<String>,
    /// Preferred content language code; use an available code from /api/cards/languages.
    /// Null until chosen. In a backup import, null keeps the saved language.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schema(required, pattern = "^[a-z]{3}$")]
    pub translation_language: Option<String>,
    pub auto_progress_on_correct: bool,
    #[schema(minimum = 0, maximum = 3000)]
    /// Delay before advancing after a correct answer, in milliseconds.
    pub auto_progress_delay: i64,
    #[schema(minimum = 0.5, maximum = 0.99)]
    pub desired_retention: f64,
    #[schema(minimum = 0)]
    pub daily_new_card_limit: i64,
    pub history_colorized_area: bool,
    pub history_colored_dots: bool,
    pub history_threshold_lines: bool,
}

/// Shared bounds for partial settings updates and full data imports.
/// Red must stay strictly below yellow, or the history colors overlap.
pub(crate) fn validate_threshold_order(red: i64, yellow: i64) -> Result<(), AppError> {
    if red >= yellow {
        return Err(AppError::BadRequest(
            "red_threshold must be below yellow_threshold".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_settings_values(
    red_threshold: Option<i64>,
    yellow_threshold: Option<i64>,
    day_boundary_hour: Option<i64>,
    auto_progress_delay: Option<i64>,
    desired_retention: Option<f64>,
    daily_new_card_limit: Option<i64>,
) -> Result<(), AppError> {
    for (name, value) in [
        ("red_threshold", red_threshold),
        ("yellow_threshold", yellow_threshold),
    ] {
        if let Some(v) = value
            && !(0..=100).contains(&v)
        {
            return Err(AppError::BadRequest(format!(
                "{name} must be between 0 and 100"
            )));
        }
    }
    if let (Some(red), Some(yellow)) = (red_threshold, yellow_threshold) {
        validate_threshold_order(red, yellow)?;
    }
    if let Some(v) = day_boundary_hour
        && !(0..=23).contains(&v)
    {
        return Err(AppError::BadRequest(
            "day_boundary_hour must be between 0 and 23".to_string(),
        ));
    }
    if let Some(v) = auto_progress_delay
        && !(0..=3000).contains(&v)
    {
        return Err(AppError::BadRequest(
            "auto_progress_delay must be between 0 and 3000".to_string(),
        ));
    }
    if let Some(v) = desired_retention
        && !(0.5..=0.99).contains(&v)
    {
        return Err(AppError::BadRequest(
            "desired_retention must be between 0.5 and 0.99".to_string(),
        ));
    }
    if let Some(v) = daily_new_card_limit
        && v.is_negative()
    {
        return Err(AppError::BadRequest(
            "new daily card limit must be a positive integer".to_string(),
        ));
    }

    Ok(())
}

#[derive(Serialize, ToSchema)]
pub struct UserSettings {
    #[serde(flatten)]
    pub core: UserSettingsCore,
    /// True when personal FSRS parameters have been fitted; false uses library defaults.
    pub has_fsrs_parameters: bool,
}

/// Partial settings update. Omitted fields and explicit null leave the saved value unchanged;
/// null does not clear the saved timezone or translation language.
#[derive(Deserialize, ToSchema)]
pub struct UpdateSettingsRequest {
    pub show_hanja_hints: Option<bool>,
    /// History accuracy percentage below which days are shown red. Must stay
    /// below the yellow threshold, including the saved one when only this is sent.
    #[schema(minimum = 0, maximum = 100)]
    pub red_threshold: Option<i64>,
    /// History accuracy percentage below which days are shown yellow. Must stay
    /// above the red threshold, including the saved one when only this is sent.
    #[schema(minimum = 0, maximum = 100)]
    pub yellow_threshold: Option<i64>,
    #[schema(minimum = 0, maximum = 23)]
    pub day_boundary_hour: Option<i64>,
    pub study_timezone: Option<String>,
    #[serde(default)]
    #[schema(pattern = "^[a-z]{3}$")]
    /// Preferred content language code; use an available code from /api/cards/languages.
    pub translation_language: Option<String>,
    /// Browser default, applied only if no timezone has been saved yet.
    pub initial_study_timezone: Option<String>,
    pub auto_progress_on_correct: Option<bool>,
    #[schema(minimum = 0, maximum = 3000)]
    /// Delay before advancing after a correct answer, in milliseconds.
    pub auto_progress_delay: Option<i64>,
    #[schema(minimum = 0.5, maximum = 0.99)]
    pub desired_retention: Option<f64>,
    #[schema(minimum = 0)]
    pub daily_new_card_limit: Option<i64>,
    pub history_colorized_area: Option<bool>,
    pub history_colored_dots: Option<bool>,
    pub history_threshold_lines: Option<bool>,
}

// Get current user's profile
#[utoipa::path(
    get,
    path = "/api/user/me",
    description = "Return the signed-in user's username and admin status. The username is null for passkey-only accounts.",
    responses(
        (status = 200, description = "Current user's profile", body = UserProfile),
        ),
    security(("cookie_auth" = [])),
    tag = "user"
)]
pub async fn get_profile(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<UserProfile>, AppError> {
    let user_id = auth.0;
    info!("Getting profile for user_id: {}", user_id);

    let (username, is_admin): (Option<String>, bool) =
        sqlx::query_as("SELECT username, is_admin FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&pool)
            .await?;

    Ok(Json(UserProfile { username, is_admin }))
}

// Get user settings
#[utoipa::path(
    get,
    path = "/api/user/settings",
    description = "Return the current user's study and display settings and whether personal FSRS parameters are stored. Creates default settings on first access if needed.",
    responses(
        (status = 200, description = "Current user settings (row lazily created on first access)", body = UserSettings),
        ),
    security(("cookie_auth" = [])),
    tag = "user"
)]
pub async fn get_settings(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<UserSettings>, AppError> {
    let user_id = auth.0;
    info!("Getting settings for user_id: {}", user_id);

    ensure_settings_row(&pool, user_id).await?;
    Ok(Json(load_settings(&pool, user_id).await?))
}

async fn load_settings(pool: &SqlitePool, user_id: i64) -> Result<UserSettings, AppError> {
    let core = sqlx::query_as::<_, UserSettingsCore>(
        r#"
        SELECT show_hanja_hints, red_threshold, yellow_threshold, day_boundary_hour, study_timezone, translation_language, auto_progress_on_correct, auto_progress_delay, desired_retention, daily_new_card_limit, history_colorized_area, history_colored_dots, history_threshold_lines
        FROM users_settings
        WHERE user_id = ?
        "#
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    // Check if user has custom FSRS parameters
    let has_fsrs_parameters: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users_fsrs_parameters WHERE user_id = ?)")
            .bind(user_id)
            .fetch_one(pool)
            .await?;

    Ok(UserSettings {
        core,
        has_fsrs_parameters,
    })
}

// Update user settings
#[utoipa::path(
    patch,
    path = "/api/user/settings",
    description = "Update only the supplied study and display settings, validating values and saving the changes atomically. The initial study timezone is applied only if none is saved yet.",
    request_body = UpdateSettingsRequest,
    responses(
        (status = 200, description = "Settings updated (only provided fields are written); returns the full saved settings", body = UserSettings),
        (status = 400, description = "Out-of-range value or malformed request", body = crate::error::ErrorResponse),
        ),
    security(("cookie_auth" = [])),
    tag = "user"
)]
pub async fn update_settings(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    AppJson(payload): AppJson<UpdateSettingsRequest>,
) -> Result<Json<UserSettings>, AppError> {
    let user_id = auth.0;
    info!("Updating settings for user_id: {}", user_id);

    let UpdateSettingsRequest {
        show_hanja_hints,
        red_threshold,
        yellow_threshold,
        day_boundary_hour,
        study_timezone,
        translation_language,
        initial_study_timezone,
        auto_progress_on_correct,
        auto_progress_delay,
        desired_retention,
        daily_new_card_limit,
        history_colorized_area,
        history_colored_dots,
        history_threshold_lines,
    } = payload;

    if let Some(code) = &translation_language {
        crate::translation_language::validate_code(code)?;
        crate::translation_language::TranslationLanguage(code.clone())
            .id(&pool)
            .await?;
    }
    validate_settings_values(
        red_threshold,
        yellow_threshold,
        day_boundary_hour,
        auto_progress_delay,
        desired_retention,
        daily_new_card_limit,
    )?;

    for name in [study_timezone.as_deref(), initial_study_timezone.as_deref()]
        .into_iter()
        .flatten()
    {
        parse_study_timezone(Some(name))?;
    }

    // One transaction for the lazy row creation and the update itself, and
    // one dynamically-built UPDATE for every provided field - same pattern
    // as admin::edit_card's cards/cards_translations updates - rather than
    // multiple separate unguarded statements against the pool. Atomicity
    // matters here specifically because there used to be none: a failure
    // partway through the old field-by-field version could leave settings
    // half-applied.
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    ensure_settings_row(&mut *tx, user_id).await?;
    // A partial update must keep the thresholds ordered against the saved values.
    if red_threshold.is_some() || yellow_threshold.is_some() {
        let (saved_red, saved_yellow): (i64, i64) = sqlx::query_as(
            "SELECT red_threshold, yellow_threshold FROM users_settings WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
        validate_threshold_order(
            red_threshold.unwrap_or(saved_red),
            yellow_threshold.unwrap_or(saved_yellow),
        )?;
    }

    let mut sets: Vec<&str> = Vec::new();
    if show_hanja_hints.is_some() {
        sets.push("show_hanja_hints = ?")
    }
    if red_threshold.is_some() {
        sets.push("red_threshold = ?")
    }
    if yellow_threshold.is_some() {
        sets.push("yellow_threshold = ?")
    }
    if day_boundary_hour.is_some() {
        sets.push("day_boundary_hour = ?")
    }
    if translation_language.is_some() {
        sets.push("translation_language = ?")
    }
    if study_timezone.is_some() {
        sets.push("study_timezone = ?")
    } else if initial_study_timezone.is_some() {
        sets.push("study_timezone = COALESCE(study_timezone, ?)")
    }
    if auto_progress_on_correct.is_some() {
        sets.push("auto_progress_on_correct = ?")
    }
    if auto_progress_delay.is_some() {
        sets.push("auto_progress_delay = ?")
    }
    if desired_retention.is_some() {
        sets.push("desired_retention = ?")
    }
    if daily_new_card_limit.is_some() {
        sets.push("daily_new_card_limit = ?")
    }
    if history_colorized_area.is_some() {
        sets.push("history_colorized_area = ?")
    }
    if history_colored_dots.is_some() {
        sets.push("history_colored_dots = ?")
    }
    if history_threshold_lines.is_some() {
        sets.push("history_threshold_lines = ?")
    }

    if !sets.is_empty() {
        let sql = format!(
            "UPDATE users_settings SET {} WHERE user_id = ?",
            sets.join(", ")
        );
        let mut q = sqlx::query(&sql);
        if let Some(v) = show_hanja_hints {
            q = q.bind(v)
        }
        if let Some(v) = red_threshold {
            q = q.bind(v)
        }
        if let Some(v) = yellow_threshold {
            q = q.bind(v)
        }
        if let Some(v) = day_boundary_hour {
            q = q.bind(v)
        }
        if let Some(v) = translation_language {
            q = q.bind(v)
        }
        if let Some(v) = study_timezone.or(initial_study_timezone) {
            q = q.bind(v)
        }
        if let Some(v) = auto_progress_on_correct {
            q = q.bind(v)
        }
        if let Some(v) = auto_progress_delay {
            q = q.bind(v)
        }
        if let Some(v) = desired_retention {
            q = q.bind(v)
        }
        if let Some(v) = daily_new_card_limit {
            q = q.bind(v)
        }
        if let Some(v) = history_colorized_area {
            q = q.bind(v)
        }
        if let Some(v) = history_colored_dots {
            q = q.bind(v)
        }
        if let Some(v) = history_threshold_lines {
            q = q.bind(v)
        }
        q.bind(user_id).execute(&mut *tx).await?;
    }

    if desired_retention.is_some() {
        crate::cards::scheduling::reschedule_user(&mut tx, user_id).await?;
    }
    tx.commit().await?;

    info!("Settings updated successfully");

    Ok(Json(load_settings(&pool, user_id).await?))
}

#[cfg(test)]
#[path = "tests/user.rs"]
mod tests;
