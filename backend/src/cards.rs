use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{Timelike, Utc};
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
    card_id: i64,
    word: String,
    definition: Option<String>,
    pos: Option<String>,
    origin_type: Option<String>,
    hanja: Option<String>,
    hanja_eum: Option<String>,
    grade: Option<String>,
    trans_word: String,
    trans_dfn: Option<String>,
    sentence: String,
    sentence_translation: String,
    target: String,
    speech_level: Option<String>,
    tense: Option<String>,
    difficulty: Option<f64>,
    guess_count: i64,
    wrong_guess_count: i64,
}

#[derive(Serialize)]
pub struct NextCardEnvelope {
    pub card: Option<NextCardResponse>,
    pub next_due_at: Option<String>,
}

#[derive(Serialize)]
pub struct SuppressedCard {
    card_id: i64,
    word: String,
    trans_word: String,
    sentence: String,
    sentence_translation: String,
    pos: Option<String>,
    grade: Option<String>,
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
) -> Result<Json<NextCardEnvelope>, AppError> {
    let user_id = auth.0;
    info!(
        "Getting next card for user_id: {} (exclude: {:?})",
        user_id, params.exclude
    );

    // Get user settings
    let user_row = sqlx::query(
        "SELECT suppress_new_cards FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?;

    let suppress_new_cards = user_row
        .and_then(|r| r.get::<Option<i64>, _>("suppress_new_cards"))
        .map(|v| v != 0)
        .unwrap_or(false);

    // Get next due card (prioritize due cards by due date, then new cards)
    // Exclude suspended cards via user_card_flags
    // Optionally skip a specific card_id (used for client-side prefetch)
    // When suppress_new_cards is enabled, exclude never-reviewed cards
    let exclude_id = params.exclude.unwrap_or(-1);
    
    let new_card_filter = if suppress_new_cards {
        "AND cs.last_review IS NOT NULL"
    } else {
        ""
    };

    let query = format!(
        r#"
        SELECT
            c.id, c.word, c.definition, c.pos, c.origin_type, c.hanja, c.hanja_eum, c.grade,
            ct.trans_word, ct.trans_dfn,
            s.text as sentence, s.target,
            st.translation as sentence_translation,
            sih.speech_level, sih.tense,
            cs.difficulty, cs.last_review, cs.stability
        FROM cards c
        LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        INNER JOIN card_translations ct ON c.id = ct.card_id AND ct.language_tag = 'en'
        INNER JOIN sentences s ON c.id = s.card_id
        LEFT JOIN sentence_translations st ON s.id = st.sentence_id
        LEFT JOIN sentence_inflection_hints sih ON s.id = sih.sentence_id
        LEFT JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
        {}
        AND (ucf.suspended IS NULL OR ucf.suspended = 0)
        AND c.id != ?
        AND (
            cs.last_review IS NULL
            OR datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days') <= datetime('now')
        )
        ORDER BY
            CASE WHEN cs.last_review IS NULL THEN 1 ELSE 0 END,
            datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days') ASC,
            c.frequency_rank ASC NULLS LAST,
            RANDOM()
        LIMIT 1
        "#,
        new_card_filter
    );

    let row = sqlx::query(&query)
        .bind(user_id)
        .bind(user_id)
        .bind(user_id)
        .bind(exclude_id)
        .fetch_optional(&pool)
        .await?;

    let Some(row) = row else {
        // No card available — find when the next one becomes due
        let next_due_at: Option<String> = sqlx::query_scalar(
            r#"
            SELECT MIN(datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days'))
            FROM cards c
            LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
            INNER JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
            LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
            WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
            AND datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days') > datetime('now')
            AND (ucf.suspended IS NULL OR ucf.suspended = 0)
            "#,
        )
        .bind(user_id)
        .bind(user_id)
        .bind(user_id)
        .fetch_optional(&pool)
        .await?;

        return Ok(Json(NextCardEnvelope {
            card: None,
            next_due_at,
        }));
    };

    let card_id: i64 = row.get("id");
    let word: String = row.get("word");
    let definition: Option<String> = row.get("definition");
    let pos: Option<String> = row.get("pos");
    let origin_type: Option<String> = row.get("origin_type");
    let hanja: Option<String> = row.get("hanja");
    let hanja_eum: Option<String> = row.get("hanja_eum");
    let grade: Option<String> = row.get("grade");
    let trans_word: String = row.get("trans_word");
    let trans_dfn: Option<String> = row.get("trans_dfn");
    let sentence: String = row.get("sentence");
    let sentence_translation: String = row.get("sentence_translation");
    let target: String = row.get("target");
    let speech_level: Option<String> = row.get("speech_level");
    let tense: Option<String> = row.get("tense");

    debug!("Selected card_id: {} ({})", card_id, word);

    // Get correct/wrong stats for this card
    let stats_row = sqlx::query(
        r#"
        SELECT
            COUNT(*) as total,
            SUM(CASE WHEN rating IN ('good', 'easy') THEN 1 ELSE 0 END) as correct
        FROM review_history
        WHERE user_id = ? AND card_id = ?
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .fetch_one(&pool)
    .await?;

    let guess_count: i64 = stats_row.get("total");
    let correct_count: i64 = stats_row.get("correct");
    let wrong_guess_count = guess_count - correct_count;

    // Get difficulty from FSRS (range 1-10)
    let difficulty: Option<f64> = if guess_count > 0 {
        row.get("difficulty")
    } else {
        None
    };

    Ok(Json(NextCardEnvelope {
        card: Some(NextCardResponse {
            card_id,
            word,
            definition,
            pos,
            origin_type,
            hanja,
            hanja_eum,
            grade,
            trans_word,
            trans_dfn,
            sentence,
            sentence_translation,
            target,
            speech_level,
            tense,
            difficulty,
            guess_count,
            wrong_guess_count,
        }),
        next_due_at: None,
    }))
}

// Submit a review for a card
pub async fn submit_review(
    State(pool): State<SqlitePool>,
    Path(card_id): Path<i64>,
    auth: crate::auth::AuthUser,
    Json(payload): Json<ReviewRequest>,
) -> Result<Json<ReviewResponse>, AppError> {
    let user_id = auth.0;
    info!(
        "Submitting review for user_id: {}, card_id: {}, rating: {}",
        user_id, card_id, payload.rating
    );

    // Map rating to FSRS scale and string representation
    // We only use 1 (Again) and 3 (Good) in the UI
    let (rating, rating_str) = match payload.rating {
        1 => (1, "again"),
        2 => (2, "hard"),
        3 => (3, "good"),
        4 => (4, "easy"),
        _ => return Err(AppError::Internal("Invalid rating".to_string())),
    };

    // Get existing card state if any
    let card_state_row = sqlx::query(
        "SELECT stability, difficulty, last_review
         FROM card_states
         WHERE user_id = ? AND card_id = ?",
    )
    .bind(user_id)
    .bind(card_id)
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
        // Existing card - load state if stability and difficulty are not NULL
        let stability: Option<f64> = row.get("stability");
        let difficulty: Option<f64> = row.get("difficulty");
        let last_review: Option<String> = row.get("last_review");

        if let (Some(stability), Some(difficulty), Some(last_review_str)) = (stability, difficulty, last_review) {
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
            // Row exists but FSRS state is NULL (suppressed new card) - treat as new
            (None, 0)
        }
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

    // Calculate scheduled days for tracking
    let scheduled_days = scheduled_state.interval;
    let now = Utc::now();

    // Determine new state based on rating
    let new_state = if memory_state.is_none() {
        "learning"
    } else if rating == 1 {
        "relearning"
    } else {
        "review"
    };

    // Update or insert card state
    sqlx::query(
        r#"
        INSERT INTO card_states (user_id, card_id, stability, difficulty, last_review, state)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, card_id) DO UPDATE SET
            stability = excluded.stability,
            difficulty = excluded.difficulty,
            last_review = excluded.last_review,
            state = excluded.state
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .bind(scheduled_state.memory.stability as f64)
    .bind(scheduled_state.memory.difficulty as f64)
    .bind(now.to_rfc3339())
    .bind(new_state)
    .execute(&pool)
    .await?;

    // Insert into review_history with full FSRS metadata
    sqlx::query(
        r#"
        INSERT INTO review_history (user_id, card_id, rating, scheduled_days, elapsed_days, stability, difficulty, state)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .bind(rating_str)
    .bind(scheduled_days as f64)
    .bind(elapsed_days as f64)
    .bind(scheduled_state.memory.stability as f64)
    .bind(scheduled_state.memory.difficulty as f64)
    .bind(new_state)
    .execute(&pool)
    .await?;

    info!("Review submitted successfully");

    Ok(Json(ReviewResponse { success: true }))
}

// Get statistics
pub async fn get_stats(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<StatsResponse>, AppError> {
    let user_id = auth.0;

    // Get suppress_new_cards setting
    let user_row = sqlx::query(
        "SELECT suppress_new_cards FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?;

    let suppress_new_cards = user_row
        .and_then(|r| r.get::<Option<i64>, _>("suppress_new_cards"))
        .map(|v| v != 0)
        .unwrap_or(false);

    // Get day_boundary_hour from user_settings (default to 4)
    let day_boundary_hour: i64 = sqlx::query_scalar(
        "SELECT day_boundary_hour FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .unwrap_or(4);

    // Calculate the "today" start timestamp
    let now = Utc::now();
    let today_start = if now.hour() >= day_boundary_hour as u32 {
        now.date_naive().and_hms_opt(day_boundary_hour as u32, 0, 0).unwrap()
    } else {
        (now.date_naive() - chrono::Duration::days(1))
            .and_hms_opt(day_boundary_hour as u32, 0, 0)
            .unwrap()
    };
    let today_start = chrono::DateTime::<Utc>::from_naive_utc_and_offset(today_start, Utc);

    // Count new cards (cards not in card_states, excluding suspended)
    let new_count_query = if suppress_new_cards {
        // When suppress_new_cards is enabled, don't count never-reviewed cards
        r#"
        SELECT COUNT(*)
        FROM cards c
        LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        INNER JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
        AND cs.last_review IS NOT NULL
        AND (ucf.suspended IS NULL OR ucf.suspended = 0)
        "#
    } else {
        r#"
        SELECT COUNT(*)
        FROM cards c
        LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        LEFT JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
        AND (cs.last_review IS NULL)
        AND (ucf.suspended IS NULL OR ucf.suspended = 0)
        "#
    };

    let new_count: i64 = sqlx::query_scalar(new_count_query)
        .bind(user_id)
        .bind(user_id)
        .bind(user_id)
        .fetch_one(&pool)
        .await?;

    // Count due cards (existing cards with last_review set, excluding suspended)
    let due_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cards c
        LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        INNER JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
        AND cs.last_review IS NOT NULL
        AND (ucf.suspended IS NULL OR ucf.suspended = 0)
        "#,
    )
    .bind(user_id)
    .bind(user_id)
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    // Count reviews today (after day_boundary_hour)
    let reviews_today: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM review_history
        WHERE user_id = ?
        AND datetime(reviewed_at) >= datetime(?)
        "#,
    )
    .bind(user_id)
    .bind(today_start.to_rfc3339())
    .fetch_one(&pool)
    .await?;

    // Count correct reviews today (rating = 'good' or 'easy')
    let correct_today: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM review_history
        WHERE user_id = ?
        AND rating IN ('good', 'easy')
        AND datetime(reviewed_at) >= datetime(?)
        "#,
    )
    .bind(user_id)
    .bind(today_start.to_rfc3339())
    .fetch_one(&pool)
    .await?;

    // Calculate percentage
    let percentage = if reviews_today > 0 {
        Some((correct_today * 100) / reviews_today)
    } else {
        None
    };

    // Find when the next card becomes due
    let next_due_at: Option<String> = sqlx::query_scalar(
        r#"
        SELECT MIN(datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days'))
        FROM cards c
        LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        INNER JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
        AND datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days') > datetime('now')
        AND (ucf.suspended IS NULL OR ucf.suspended = 0)
        "#,
    )
    .bind(user_id)
    .bind(user_id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await?;

    Ok(Json(StatsResponse {
        new_count,
        due_count,
        reviews_today,
        correct_today,
        percentage,
        next_due_at,
    }))
}

pub async fn suppress_card(
    State(pool): State<SqlitePool>,
    Path(card_id): Path<i64>,
    auth: crate::auth::AuthUser,
) -> Result<Json<ReviewResponse>, AppError> {
    let user_id = auth.0;
    info!("Suppressing card for user_id: {}, card_id: {}", user_id, card_id);

    // Insert or update user_card_flags to mark as suspended
    sqlx::query(
        r#"
        INSERT INTO user_card_flags (user_id, card_id, suspended)
        VALUES (?, ?, 1)
        ON CONFLICT(user_id, card_id) DO UPDATE SET
            suspended = 1,
            flagged_at = datetime('now')
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .execute(&pool)
    .await?;

    info!("Card suspended successfully");

    Ok(Json(ReviewResponse { success: true }))
}

// List all suspended cards for the user
pub async fn list_suppressed_cards(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<SuppressedCardsResponse>, AppError> {
    let user_id = auth.0;
    info!("Listing suspended cards for user_id: {}", user_id);

    let rows = sqlx::query(
        r#"
        SELECT
            c.id, c.word, c.pos, c.grade,
            ct.trans_word,
            s.text as sentence,
            st.translation as sentence_translation
        FROM cards c
        INNER JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        INNER JOIN card_translations ct ON c.id = ct.card_id AND ct.language_tag = 'en'
        INNER JOIN sentences s ON c.id = s.card_id
        LEFT JOIN sentence_translations st ON s.id = st.sentence_id
        WHERE ucf.suspended = 1
        ORDER BY c.word ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let cards: Vec<SuppressedCard> = rows
        .iter()
        .map(|row| SuppressedCard {
            card_id: row.get("id"),
            word: row.get("word"),
            trans_word: row.get("trans_word"),
            sentence: row.get("sentence"),
            sentence_translation: row.get("sentence_translation"),
            pos: row.get("pos"),
            grade: row.get("grade"),
        })
        .collect();

    Ok(Json(SuppressedCardsResponse { cards }))
}

pub async fn unsuppress_card(
    State(pool): State<SqlitePool>,
    Path(card_id): Path<i64>,
    auth: crate::auth::AuthUser,
) -> Result<Json<ReviewResponse>, AppError> {
    let user_id = auth.0;
    info!("Unsuspending card for user_id: {}, card_id: {}", user_id, card_id);

    sqlx::query(
        r#"
        UPDATE user_card_flags
        SET suspended = 0
        WHERE user_id = ? AND card_id = ?
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .execute(&pool)
    .await?;

    info!("Card unsuspended successfully");

    Ok(Json(ReviewResponse { success: true }))
}