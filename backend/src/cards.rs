use axum::{
    extract::{Path, Query, State},
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
    correct_rate: f64,
    guess_count: i64,
    wrong_guess_count: i64,
}

#[derive(Serialize)]
pub struct SuppressedCard {
    word_id: i64,
    form: String,
    hint: String,
    context: String,
    context_translation: String,
    grammar: Option<String>,
    politeness: Option<String>,
}

#[derive(Serialize)]
pub struct SuppressedCardsResponse {
    cards: Vec<SuppressedCard>,
}

#[derive(Serialize)]
pub struct ReviewResponse {
    success: bool,
}

#[derive(Serialize)]
pub struct StatsResponse {
    new_count: i64,
    due_count: i64,
    reviews_today: i64,
    correct_today: i64,
    percentage: Option<i64>,
    next_due_at: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct NextCardQuery {
    // Optional word_id to exclude from the result (used by client prefetch
    // to skip the card currently being shown to the user).
    exclude: Option<i64>,
}

// Get next card due for review
pub async fn get_next_card(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
    Query(params): Query<NextCardQuery>,
) -> Result<Json<NextCardResponse>, AppError> {
    let user_id = auth.0;
    info!(
        "Getting next card for user_id: {} (exclude: {:?})",
        user_id, params.exclude
    );

    // Get user's target language and settings
    let user_row = sqlx::query(
        "SELECT target_language_id, suppress_new_cards FROM users u LEFT JOIN user_settings us ON us.user_id = u.id WHERE u.id = ?"
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    let target_language_id: Option<i64> = user_row.get("target_language_id");
    let suppress_new_cards: Option<bool> = user_row.get("suppress_new_cards");
    let suppress_new_cards = suppress_new_cards.unwrap_or(false);

    let target_language_id = target_language_id
        .ok_or_else(|| AppError::Internal("User has no target language set".to_string()))?;

    // Get next due card (prioritize due cards by date, then new cards)
    // Filter by user's target language and exclude suppressed cards.
    // Optionally skip a specific word_id (used for client-side prefetch so
    // the prefetched card isn't the same as the one currently displayed).
    // When suppress_new_cards is enabled, exclude never-reviewed cards.
    let exclude_id = params.exclude.unwrap_or(-1);
    let new_card_filter = if suppress_new_cards {
        "AND cs.due_date IS NOT NULL AND datetime(cs.due_date) <= datetime('now')"
    } else {
        "AND (cs.due_date IS NULL OR datetime(cs.due_date) <= datetime('now'))"
    };

    let query = format!(
        r#"
        SELECT
            w.id, w.form, w.hint, w.context, w.context_translation,
            w.grammar, w.politeness, w.notes,
            cs.due_date
        FROM words w
        LEFT JOIN card_states cs ON cs.word_id = w.id AND cs.user_id = ?
        WHERE w.language_id = ?
        AND (w.user_id IS NULL OR w.user_id = ?)
        {}
        AND (cs.suppressed IS NULL OR cs.suppressed = 0)
        AND w.id != ?
        ORDER BY
            CASE WHEN cs.due_date IS NULL THEN 1 ELSE 0 END,
            cs.due_date ASC
        LIMIT 1
        "#,
        new_card_filter
    );

    let row = sqlx::query(&query)
    .bind(user_id)
    .bind(target_language_id)
    .bind(user_id)
    .bind(exclude_id)
    .fetch_optional(&pool)
    .await?;

    let row = row.ok_or(AppError::NoCardsDue)?;

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

    // Get card statistics from review history
    let stats = sqlx::query(
        r#"
        SELECT
            COUNT(*) as guess_count,
            SUM(CASE WHEN rating < 3 THEN 1 ELSE 0 END) as wrong_guess_count
        FROM review_history
        WHERE user_id = ? AND word_id = ?
        "#,
    )
    .bind(user_id)
    .bind(word_id)
    .fetch_one(&pool)
    .await?;

    let guess_count: i64 = stats.get("guess_count");
    let wrong_guess_count: i64 = stats.get("wrong_guess_count");

    let correct_rate = if guess_count > 0 {
        ((guess_count - wrong_guess_count) as f64 / guess_count as f64) * 100.0
    } else {
        0.0
    };

    Ok(Json(NextCardResponse {
        word_id,
        form,
        hint,
        context,
        context_translation,
        grammar,
        politeness,
        notes,
        correct_rate,
        guess_count,
        wrong_guess_count,
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

    // Fetch user's desired retention setting
    let desired_retention: f64 = sqlx::query_scalar(
        "SELECT desired_retention FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .unwrap_or(0.9);

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
        .next_states(memory_state, desired_retention as f32, elapsed_days)
        .map_err(|e| AppError::Internal(format!("FSRS error: {:?}", e)))?;

    // Select the appropriate state based on rating
    let scheduled_state = match rating {
        1 => next_states.again,
        2 => next_states.hard,
        3 => next_states.good,
        4 => next_states.easy,
        _ => next_states.good,
    };

    // new — matches Anki: whole days, minimum 1
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

// Get stats about due cards
pub async fn get_stats(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<StatsResponse>, AppError> {
    let user_id = auth.0;

    // Get user's target language
    let target_language_id: Option<i64> = sqlx::query_scalar(
        "SELECT target_language_id FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    let target_language_id = target_language_id
        .ok_or_else(|| AppError::Internal("User has no target language set".to_string()))?;

    // Count new cards (never seen), excluding suppressed
    let new_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM words w
        LEFT JOIN card_states cs ON cs.word_id = w.id AND cs.user_id = ?
        WHERE w.language_id = ?
        AND (w.user_id IS NULL OR w.user_id = ?)
        AND cs.due_date IS NULL
        AND (cs.suppressed IS NULL OR cs.suppressed = 0)
        "#,
    )
    .bind(user_id)
    .bind(target_language_id)
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    // Count due cards (seen and due), excluding suppressed
    let due_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM words w
        INNER JOIN card_states cs ON cs.word_id = w.id AND cs.user_id = ?
        WHERE w.language_id = ?
        AND (w.user_id IS NULL OR w.user_id = ?)
        AND datetime(cs.due_date) <= datetime('now')
        AND (cs.suppressed IS NULL OR cs.suppressed = 0)
        "#,
    )
    .bind(user_id)
    .bind(target_language_id)
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    // Get user's day boundary setting
    let day_boundary_hour: i64 = sqlx::query_scalar(
        "SELECT day_boundary_hour FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .unwrap_or(4);

    // Get today's review stats (adjusted for user's day boundary)
    let reviews_today: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM review_history
        WHERE user_id = ?
        AND DATE(datetime(reviewed_at, printf('-%d hours', ?))) = DATE(datetime('now', printf('-%d hours', ?)))
        "#,
    )
    .bind(user_id)
    .bind(day_boundary_hour)
    .bind(day_boundary_hour)
    .fetch_one(&pool)
    .await?;

    let correct_today: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM review_history
        WHERE user_id = ?
        AND DATE(datetime(reviewed_at, printf('-%d hours', ?))) = DATE(datetime('now', printf('-%d hours', ?)))
        AND rating >= 3
        "#,
    )
    .bind(user_id)
    .bind(day_boundary_hour)
    .bind(day_boundary_hour)
    .fetch_one(&pool)
    .await?;

    let percentage = if reviews_today > 0 {
        Some((correct_today * 100) / reviews_today)
    } else {
        None
    };

    // Get the next card's due date (when no reviewed cards are due)
    let next_due_at: Option<String> = if due_count == 0 {
        sqlx::query_scalar(
            r#"
            SELECT MIN(cs.due_date)
            FROM words w
            INNER JOIN card_states cs ON cs.word_id = w.id AND cs.user_id = ?
            WHERE w.language_id = ?
            AND (w.user_id IS NULL OR w.user_id = ?)
            AND datetime(cs.due_date) > datetime('now')
            AND (cs.suppressed IS NULL OR cs.suppressed = 0)
            "#,
        )
        .bind(user_id)
        .bind(target_language_id)
        .bind(user_id)
        .fetch_optional(&pool)
        .await?
    } else {
        None
    };

    Ok(Json(StatsResponse {
        new_count,
        due_count,
        reviews_today,
        correct_today,
        percentage,
        next_due_at,
    }))
}

// Suppress a card (don't show again)
pub async fn suppress_card(
    State(pool): State<SqlitePool>,
    Path(word_id): Path<i64>,
    auth: crate::auth::AuthUser,
) -> Result<Json<ReviewResponse>, AppError> {
    let user_id = auth.0;
    info!("Suppressing card for user_id: {}, word_id: {}", user_id, word_id);

    // Insert or update card_states to mark as suppressed
    sqlx::query(
        r#"
        INSERT INTO card_states (user_id, word_id, stability, difficulty, last_review, due_date, suppressed)
        VALUES (?, ?, 0.0, 0.0, datetime('now'), datetime('now'), 1)
        ON CONFLICT(user_id, word_id) DO UPDATE SET
            suppressed = 1
        "#,
    )
    .bind(user_id)
    .bind(word_id)
    .execute(&pool)
    .await?;

    info!("Card suppressed successfully");

    Ok(Json(ReviewResponse { success: true }))
}

// List all suppressed cards for the user
pub async fn list_suppressed_cards(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<SuppressedCardsResponse>, AppError> {
    let user_id = auth.0;
    info!("Listing suppressed cards for user_id: {}", user_id);

    let rows = sqlx::query(
        r#"
        SELECT
            w.id, w.form, w.hint, w.context, w.context_translation,
            w.grammar, w.politeness
        FROM words w
        INNER JOIN card_states cs ON cs.word_id = w.id AND cs.user_id = ?
        WHERE cs.suppressed = 1
        ORDER BY w.form ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let cards: Vec<SuppressedCard> = rows
        .iter()
        .map(|row| SuppressedCard {
            word_id: row.get("id"),
            form: row.get("form"),
            hint: row.get("hint"),
            context: row.get("context"),
            context_translation: row.get("context_translation"),
            grammar: row.get("grammar"),
            politeness: row.get("politeness"),
        })
        .collect();

    info!("Found {} suppressed cards", cards.len());

    Ok(Json(SuppressedCardsResponse { cards }))
}

// Unsuppress a card (restore it to the review queue)
pub async fn unsuppress_card(
    State(pool): State<SqlitePool>,
    Path(word_id): Path<i64>,
    auth: crate::auth::AuthUser,
) -> Result<Json<ReviewResponse>, AppError> {
    let user_id = auth.0;
    info!("Unsuppressing card for user_id: {}, word_id: {}", user_id, word_id);

    // Update card_states to mark as not suppressed
    sqlx::query(
        r#"
        UPDATE card_states
        SET suppressed = 0
        WHERE user_id = ? AND word_id = ?
        "#,
    )
    .bind(user_id)
    .bind(word_id)
    .execute(&pool)
    .await?;

    info!("Card unsuppressed successfully");

    Ok(Json(ReviewResponse { success: true }))
}
