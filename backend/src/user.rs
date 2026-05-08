use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::info;

use crate::error::AppError;
use crate::auth::AuthUser;

#[derive(Serialize)]
pub struct UserProfile {
    pub username: String,
}

#[derive(Serialize, Deserialize)]
pub struct UserSettings {
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

#[derive(Deserialize)]
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



#[derive(Serialize)]
pub struct UpdateSettingsResponse {
    pub success: bool,
}

// Get current user's profile
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
pub async fn get_settings(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<UserSettings>, AppError> {
    let user_id = auth.0;
    info!("Getting settings for user_id: {}", user_id);

    // Ensure user_settings row exists
    sqlx::query(
        r#"
        INSERT INTO user_settings (user_id)
        VALUES (?)
        ON CONFLICT(user_id) DO NOTHING
        "#
    )
    .bind(user_id)
    .execute(&pool)
    .await?;

    let row = sqlx::query(
        r#"
        SELECT show_percentage, red_threshold, yellow_threshold, day_boundary_hour, auto_progress_on_correct, auto_progress_delay, desired_retention, daily_new_card_limit, history_colorized_area, history_colored_dots, history_threshold_lines
        FROM user_settings
        WHERE user_id = ?
        "#
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    Ok(Json(UserSettings {
        show_percentage: row.get("show_percentage"),
        red_threshold: row.get("red_threshold"),
        yellow_threshold: row.get("yellow_threshold"),
        day_boundary_hour: row.get("day_boundary_hour"),
        auto_progress_on_correct: row.get("auto_progress_on_correct"),
        auto_progress_delay: row.get("auto_progress_delay"),
        desired_retention: row.get("desired_retention"),
        daily_new_card_limit: row.get("daily_new_card_limit"),
        history_colorized_area: row.get("history_colorized_area"),
        history_colored_dots: row.get("history_colored_dots"),
        history_threshold_lines: row.get("history_threshold_lines"),
    }))
}

// Update user settings
pub async fn update_settings(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Json(payload): Json<UpdateSettingsRequest>,
) -> Result<Json<UpdateSettingsResponse>, AppError> {
    let user_id = auth.0;
    info!("Updating settings for user_id: {}", user_id);

    // Ensure user_settings row exists
    sqlx::query(
        r#"
        INSERT INTO user_settings (user_id)
        VALUES (?)
        ON CONFLICT(user_id) DO NOTHING
        "#
    )
    .bind(user_id)
    .execute(&pool)
    .await?;

    // Update individual fields if provided
    if let Some(show_percentage) = payload.show_percentage {
        sqlx::query("UPDATE user_settings SET show_percentage = ? WHERE user_id = ?")
            .bind(show_percentage)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(red_threshold) = payload.red_threshold {
        sqlx::query("UPDATE user_settings SET red_threshold = ? WHERE user_id = ?")
            .bind(red_threshold)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(yellow_threshold) = payload.yellow_threshold {
        sqlx::query("UPDATE user_settings SET yellow_threshold = ? WHERE user_id = ?")
            .bind(yellow_threshold)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(day_boundary_hour) = payload.day_boundary_hour {
        // Validate hour is between 0 and 23
        if day_boundary_hour < 0 || day_boundary_hour > 23 {
            return Err(AppError::Internal("day_boundary_hour must be between 0 and 23".to_string()));
        }
        sqlx::query("UPDATE user_settings SET day_boundary_hour = ? WHERE user_id = ?")
            .bind(day_boundary_hour)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(auto_progress_on_correct) = payload.auto_progress_on_correct {
        sqlx::query("UPDATE user_settings SET auto_progress_on_correct = ? WHERE user_id = ?")
            .bind(auto_progress_on_correct)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(auto_progress_delay) = payload.auto_progress_delay {
        if auto_progress_delay < 0 || auto_progress_delay > 3000 {
            return Err(AppError::Internal("auto_progress_delay must be between 0 and 3000".to_string()));
        }
        sqlx::query("UPDATE user_settings SET auto_progress_delay = ? WHERE user_id = ?")
            .bind(auto_progress_delay)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(desired_retention) = payload.desired_retention {
        if desired_retention < 0.5 || desired_retention > 0.99 {
            return Err(AppError::Internal("desired_retention must be between 0.5 and 0.99".to_string()));
        }
        sqlx::query("UPDATE user_settings SET desired_retention = ? WHERE user_id = ?")
            .bind(desired_retention)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(daily_new_card_limit) = payload.daily_new_card_limit {
        if daily_new_card_limit < 1 || daily_new_card_limit > 999 {
            return Err(AppError::Internal("daily_new_card_limit must be between 1 and 999".to_string()));
        }
        sqlx::query("UPDATE user_settings SET daily_new_card_limit = ? WHERE user_id = ?")
            .bind(daily_new_card_limit)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(v) = payload.history_colorized_area {
        sqlx::query("UPDATE user_settings SET history_colorized_area = ? WHERE user_id = ?")
            .bind(v)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(v) = payload.history_colored_dots {
        sqlx::query("UPDATE user_settings SET history_colored_dots = ? WHERE user_id = ?")
            .bind(v)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(v) = payload.history_threshold_lines {
        sqlx::query("UPDATE user_settings SET history_threshold_lines = ? WHERE user_id = ?")
            .bind(v)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    info!("Settings updated successfully");

    Ok(Json(UpdateSettingsResponse { success: true }))
}
