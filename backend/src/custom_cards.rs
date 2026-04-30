use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::info;

use crate::error::AppError;
use crate::auth::AuthUser;

#[derive(Deserialize)]
pub struct CreateCustomCardRequest {
    pub form: String,
    pub hint: String,
    pub context: String,
    pub context_translation: String,
    pub grammar: Option<String>,
    pub politeness: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Serialize)]
pub struct CreateCustomCardResponse {
    pub id: i64,
    pub success: bool,
}

#[derive(Serialize)]
pub struct CustomCard {
    pub id: i64,
    pub form: String,
    pub hint: String,
    pub context: String,
    pub context_translation: String,
    pub grammar: Option<String>,
    pub politeness: Option<String>,
    pub notes: Vec<String>,
    pub created_at: String,
    pub language_id: i64,
}

#[derive(Serialize)]
pub struct ListCustomCardsResponse {
    pub cards: Vec<CustomCard>,
}

#[derive(Serialize)]
pub struct DeleteCustomCardResponse {
    pub success: bool,
}

// Create a new custom card
pub async fn create_custom_card(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Json(payload): Json<CreateCustomCardRequest>,
) -> Result<Json<CreateCustomCardResponse>, AppError> {
    let user_id = auth.0;
    info!("Creating custom card for user_id: {}", user_id);

    // Get user's target language
    let target_language_id: Option<i64> = sqlx::query_scalar(
        "SELECT target_language_id FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    let target_language_id = target_language_id
        .ok_or_else(|| AppError::Internal("User has no target language set".to_string()))?;

    // Validate required fields
    if payload.form.trim().is_empty() {
        return Err(AppError::Internal("Form cannot be empty".to_string()));
    }
    if payload.hint.trim().is_empty() {
        return Err(AppError::Internal("Hint cannot be empty".to_string()));
    }
    if payload.context.trim().is_empty() {
        return Err(AppError::Internal("Context cannot be empty".to_string()));
    }
    if payload.context_translation.trim().is_empty() {
        return Err(AppError::Internal("Context translation cannot be empty".to_string()));
    }

    // Serialize notes to JSON
    let notes_json = serde_json::to_string(&payload.notes)
        .map_err(|e| AppError::Internal(format!("Failed to serialize notes: {}", e)))?;

    // Insert custom card
    let result = sqlx::query(
        r#"
        INSERT INTO words (user_id, form, hint, context, context_translation, grammar, politeness, notes, language_id, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
        "#
    )
    .bind(user_id)
    .bind(&payload.form)
    .bind(&payload.hint)
    .bind(&payload.context)
    .bind(&payload.context_translation)
    .bind(&payload.grammar)
    .bind(&payload.politeness)
    .bind(&notes_json)
    .bind(target_language_id)
    .execute(&pool)
    .await?;

    let card_id = result.last_insert_rowid();

    info!("Custom card created successfully with id: {}", card_id);

    Ok(Json(CreateCustomCardResponse {
        id: card_id,
        success: true,
    }))
}

// List all custom cards for the authenticated user
pub async fn list_custom_cards(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<ListCustomCardsResponse>, AppError> {
    let user_id = auth.0;
    info!("Listing custom cards for user_id: {}", user_id);

    // Get user's target language
    let target_language_id: Option<i64> = sqlx::query_scalar(
        "SELECT target_language_id FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    let target_language_id = target_language_id
        .ok_or_else(|| AppError::Internal("User has no target language set".to_string()))?;

    let rows = sqlx::query(
        r#"
        SELECT id, form, hint, context, context_translation, grammar, politeness, notes, language_id, 
               datetime(created_at) as created_at
        FROM words
        WHERE user_id = ? AND language_id = ?
        ORDER BY created_at DESC
        "#
    )
    .bind(user_id)
    .bind(target_language_id)
    .fetch_all(&pool)
    .await?;

    let cards = rows.into_iter().map(|row| {
        let notes_json: String = row.get("notes");
        let notes: Vec<String> = serde_json::from_str(&notes_json).unwrap_or_default();

        CustomCard {
            id: row.get("id"),
            form: row.get("form"),
            hint: row.get("hint"),
            context: row.get("context"),
            context_translation: row.get("context_translation"),
            grammar: row.get("grammar"),
            politeness: row.get("politeness"),
            notes,
            created_at: row.get("created_at"),
            language_id: row.get("language_id"),
        }
    }).collect();

    Ok(Json(ListCustomCardsResponse { cards }))
}

// Delete a custom card
pub async fn delete_custom_card(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Path(card_id): Path<i64>,
) -> Result<Json<DeleteCustomCardResponse>, AppError> {
    let user_id = auth.0;
    info!("Deleting custom card {} for user_id: {}", card_id, user_id);

    // Ensure the card belongs to the user
    let result = sqlx::query(
        "DELETE FROM words WHERE id = ? AND user_id = ?"
    )
    .bind(card_id)
    .bind(user_id)
    .execute(&pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    info!("Custom card deleted successfully");

    Ok(Json(DeleteCustomCardResponse { success: true }))
}

// Get a single custom card by ID
pub async fn get_custom_card(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Path(card_id): Path<i64>,
) -> Result<Json<CustomCard>, AppError> {
    let user_id = auth.0;
    info!("Getting custom card {} for user_id: {}", card_id, user_id);

    let row = sqlx::query(
        r#"
        SELECT id, form, hint, context, context_translation, grammar, politeness, notes, language_id,
               datetime(created_at) as created_at
        FROM words
        WHERE id = ? AND user_id = ?
        "#
    )
    .bind(card_id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await?;

    let row = row.ok_or(AppError::NotFound)?;

    let notes_json: String = row.get("notes");
    let notes: Vec<String> = serde_json::from_str(&notes_json).unwrap_or_default();

    Ok(Json(CustomCard {
        id: row.get("id"),
        form: row.get("form"),
        hint: row.get("hint"),
        context: row.get("context"),
        context_translation: row.get("context_translation"),
        grammar: row.get("grammar"),
        politeness: row.get("politeness"),
        notes,
        created_at: row.get("created_at"),
        language_id: row.get("language_id"),
    }))
}

// Update a custom card
#[derive(Deserialize)]
pub struct UpdateCustomCardRequest {
    pub form: Option<String>,
    pub hint: Option<String>,
    pub context: Option<String>,
    pub context_translation: Option<String>,
    pub grammar: Option<String>,
    pub politeness: Option<String>,
    pub notes: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct UpdateCustomCardResponse {
    pub success: bool,
}

pub async fn update_custom_card(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Path(card_id): Path<i64>,
    Json(payload): Json<UpdateCustomCardRequest>,
) -> Result<Json<UpdateCustomCardResponse>, AppError> {
    let user_id = auth.0;
    info!("Updating custom card {} for user_id: {}", card_id, user_id);

    // Verify the card exists and belongs to the user
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM words WHERE id = ? AND user_id = ?"
    )
    .bind(card_id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await?;

    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    // Update individual fields if provided
    if let Some(form) = payload.form {
        if form.trim().is_empty() {
            return Err(AppError::Internal("Form cannot be empty".to_string()));
        }
        sqlx::query("UPDATE words SET form = ? WHERE id = ?")
            .bind(&form)
            .bind(card_id)
            .execute(&pool)
            .await?;
    }

    if let Some(hint) = payload.hint {
        if hint.trim().is_empty() {
            return Err(AppError::Internal("Hint cannot be empty".to_string()));
        }
        sqlx::query("UPDATE words SET hint = ? WHERE id = ?")
            .bind(&hint)
            .bind(card_id)
            .execute(&pool)
            .await?;
    }

    if let Some(context) = payload.context {
        if context.trim().is_empty() {
            return Err(AppError::Internal("Context cannot be empty".to_string()));
        }
        sqlx::query("UPDATE words SET context = ? WHERE id = ?")
            .bind(&context)
            .bind(card_id)
            .execute(&pool)
            .await?;
    }

    if let Some(context_translation) = payload.context_translation {
        if context_translation.trim().is_empty() {
            return Err(AppError::Internal("Context translation cannot be empty".to_string()));
        }
        sqlx::query("UPDATE words SET context_translation = ? WHERE id = ?")
            .bind(&context_translation)
            .bind(card_id)
            .execute(&pool)
            .await?;
    }

    if let Some(grammar) = payload.grammar {
        sqlx::query("UPDATE words SET grammar = ? WHERE id = ?")
            .bind(&grammar)
            .bind(card_id)
            .execute(&pool)
            .await?;
    }

    if let Some(politeness) = payload.politeness {
        sqlx::query("UPDATE words SET politeness = ? WHERE id = ?")
            .bind(&politeness)
            .bind(card_id)
            .execute(&pool)
            .await?;
    }

    if let Some(notes) = payload.notes {
        let notes_json = serde_json::to_string(&notes)
            .map_err(|e| AppError::Internal(format!("Failed to serialize notes: {}", e)))?;
        sqlx::query("UPDATE words SET notes = ? WHERE id = ?")
            .bind(&notes_json)
            .bind(card_id)
            .execute(&pool)
            .await?;
    }

    info!("Custom card updated successfully");

    Ok(Json(UpdateCustomCardResponse { success: true }))
}