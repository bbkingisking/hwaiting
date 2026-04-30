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
    pub target_language: Option<LanguageInfo>,
}

#[derive(Serialize)]
pub struct LanguageInfo {
    pub id: i64,
    pub code: String,
    pub name: String,
}

#[derive(Deserialize)]
pub struct SetLanguageRequest {
    pub language_id: i64,
}

#[derive(Serialize)]
pub struct SetLanguageResponse {
    pub success: bool,
}

// Get current user's profile
pub async fn get_profile(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<UserProfile>, AppError> {
    let user_id = auth.0;
    info!("Getting profile for user_id: {}", user_id);

    let row = sqlx::query(
        r#"
        SELECT 
            u.username,
            l.id as lang_id,
            l.code as lang_code,
            l.name as lang_name
        FROM users u
        LEFT JOIN languages l ON l.id = u.target_language_id
        WHERE u.id = ?
        "#,
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    let username: String = row.get("username");
    let lang_id: Option<i64> = row.get("lang_id");
    
    let target_language = if let Some(id) = lang_id {
        Some(LanguageInfo {
            id,
            code: row.get("lang_code"),
            name: row.get("lang_name"),
        })
    } else {
        None
    };

    Ok(Json(UserProfile {
        username,
        target_language,
    }))
}

// Get list of available languages
pub async fn get_languages(
    State(pool): State<SqlitePool>,
) -> Result<Json<Vec<LanguageInfo>>, AppError> {
    info!("Getting list of available languages");

    let rows = sqlx::query("SELECT id, code, name FROM languages ORDER BY name")
        .fetch_all(&pool)
        .await?;

    let languages: Vec<LanguageInfo> = rows
        .iter()
        .map(|row| LanguageInfo {
            id: row.get("id"),
            code: row.get("code"),
            name: row.get("name"),
        })
        .collect();

    Ok(Json(languages))
}

// Set user's target language
pub async fn set_language(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Json(payload): Json<SetLanguageRequest>,
) -> Result<Json<SetLanguageResponse>, AppError> {
    let user_id = auth.0;
    info!("Setting language for user_id: {} to language_id: {}", user_id, payload.language_id);

    // Verify that the language exists
    let language_exists: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM languages WHERE id = ?"
    )
    .bind(payload.language_id)
    .fetch_optional(&pool)
    .await?;

    if language_exists.is_none() {
        return Err(AppError::Internal("Language not found".to_string()));
    }

    // Update user's target language
    sqlx::query("UPDATE users SET target_language_id = ? WHERE id = ?")
        .bind(payload.language_id)
        .bind(user_id)
        .execute(&pool)
        .await?;

    info!("Language set successfully");

    Ok(Json(SetLanguageResponse { success: true }))
}