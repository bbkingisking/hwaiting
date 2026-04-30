use axum::{
    extract::{Path, State},
    Json,
};
use chrono::Utc;
use fsrs::{MemoryState, FSRS, DEFAULT_PARAMETERS};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};

use crate::error::AppError;

#[derive(Deserialize)]
pub struct ReviewRequest {
    rating: u8, // 1 = Again, 3 = Good
}

#[derive(Serialize)]
pub struct NextCardResponse {
    word_id: i64,
    form: String,
    hint: String,
    context: String,
    context_translation: String,
    grammar: Option<String>,
    politeness: Option<String>,
    notes: Vec<String>,
}

#[derive(Serialize)]
pub struct ReviewResponse {
    success: bool,
}

// Get next card due for review
pub async fn get_next_card(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<NextCardResponse>, AppError> {
    let user_id = auth.0;
    info!("Getting next card for user_id: {}", user_id);

    // Get next due card (prioritize new cards, then due cards by date)
    let row = sqlx::query(
        r#"
        SELECT 
            w.id, w.form, w.hint, w.context, w.context_translation,
            w.grammar, w.politeness, w.notes,
            cs.due_date
        FROM words w
        LEFT JOIN card_states cs ON cs.word_id = w.id AND cs.user_id = ?
        WHERE cs.due_date IS NULL OR cs.due_date <= datetime('now')
        ORDER BY 
            CASE WHEN cs.due_date IS NULL THEN 0 ELSE 1 END,
            cs.due_date ASC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?;

    let row = row.ok_or(AppError::Internal("No cards available".to_string()))?;

    let word_id: i64 = row.get("id");
    let form: String = row.get("form");
    let hint: String = row.get("hint");
    let context: String = row.get("context");
    let context_translation: String = row.get("context_translation");
    let grammar: Option<String> = row.get("grammar");
    let politeness: Option<String> = row.get("politeness");
    let notes_json: String = row.get("notes");
    let notes: Vec<String> = serde_json::from_str(&notes_json).unwrap_or_default();

    debug!("Selected word_id: {} ({})", word_id, form);

    Ok(Json(NextCardResponse {
        word_id,
        form,
        hint,
        context,
        context_translation,
        grammar,
        politeness,
        notes,
    }))
}

// Submit a review for a card
pub async fn submit_review(
    State(pool): State<SqlitePool>,
    Path(word_id): Path<i64>,
    auth: crate::auth::AuthUser,
    Json(payload): Json<ReviewRequest>,
) -> Result<Json<ReviewResponse>, AppError> {
    let user_id = auth.0;
    info!(
        "Submitting review for user_id: {}, word_id: {}, rating: {}",
        user_id, word_id, payload.rating
    );

    // Convert rating (1 or 3) to FSRS rating (1-4 scale)
    let rating = match payload.rating {
        1 => 1, // Again
        3 => 3, // Good
        _ => return Err(AppError::Internal("Invalid rating".to_string())),
    };

    // Get existing card state if any
    let card_state_row = sqlx::query(
        "SELECT stability, difficulty, last_review 
         FROM card_states 
         WHERE user_id = ? AND word_id = ?",
    )
    .bind(user_id)
    .bind(word_id)
    .fetch_optional(&pool)
    .await?;

    let fsrs = FSRS::new(Some(&DEFAULT_PARAMETERS)).map_err(|e| AppError::Internal(format!("FSRS init error: {:?}", e)))?;
    
    // TODO: Load from user_settings.desired_retention
    let desired_retention = 0.9;

    let (memory_state, elapsed_days) = if let Some(ref row) = card_state_row {
        // Existing card - load state
        let stability: f64 = row.get("stability");
        let difficulty: f64 = row.get("difficulty");
        let last_review_str: String = row.get("last_review");

        let last_review_time = chrono::DateTime::parse_from_rfc3339(&last_review_str)
            .map_err(|e| AppError::Internal(format!("Invalid date format: {}", e)))?
            .with_timezone(&Utc);
        
        let now = Utc::now();
        let elapsed_days = (now - last_review_time).num_days().max(0) as u32;

        let state = MemoryState {
            stability: stability as f32,
            difficulty: difficulty as f32,
        };

        (Some(state), elapsed_days)
    } else {
        // New card
        (None, 0)
    };

    // Get next states from FSRS
    let next_states = fsrs
        .next_states(memory_state, desired_retention, elapsed_days)
        .map_err(|e| AppError::Internal(format!("FSRS error: {:?}", e)))?;

    // Select the appropriate state based on rating
    let scheduled_state = match rating {
        1 => next_states.again,
        2 => next_states.hard,
        3 => next_states.good,
        4 => next_states.easy,
        _ => next_states.good,
    };

    // Calculate due date
    let interval_days = scheduled_state.interval.round().max(1.0) as i64;
    let now = Utc::now();
    let due_date = now + chrono::Duration::days(interval_days);

    // Update or insert card state (only FSRS essentials)
    sqlx::query(
        r#"
        INSERT INTO card_states (user_id, word_id, stability, difficulty, last_review, due_date)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, word_id) DO UPDATE SET
            stability = excluded.stability,
            difficulty = excluded.difficulty,
            last_review = excluded.last_review,
            due_date = excluded.due_date
        "#,
    )
    .bind(user_id)
    .bind(word_id)
    .bind(scheduled_state.memory.stability as f64)
    .bind(scheduled_state.memory.difficulty as f64)
    .bind(now.to_rfc3339())
    .bind(due_date.to_rfc3339())
    .execute(&pool)
    .await?;

    // Record review in history
    sqlx::query("INSERT INTO review_history (user_id, word_id, rating) VALUES (?, ?, ?)")
        .bind(user_id)
        .bind(word_id)
        .bind(rating as i64)
        .execute(&pool)
        .await?;

    info!("Review submitted successfully. Next due: {}", due_date);

    Ok(Json(ReviewResponse { success: true }))
}