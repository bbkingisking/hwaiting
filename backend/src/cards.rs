use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{Local, TimeZone, Timelike, Utc};
use fsrs::{MemoryState, FSRS, DEFAULT_PARAMETERS};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};

use crate::error::AppError;

#[derive(Deserialize)]
pub struct ReviewRequest {
    rating: u8, // 1 = Again, 3 = Good
}

#[derive(Serialize, Clone)]
pub struct HanjaHint {
    pub hanja: String,
    pub hanja_eum: Option<String>,
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
    alternatives: Vec<String>,
    speech_level: Option<String>,
    tense: Option<String>,
    difficulty: Option<f64>,
    guess_count: i64,
    wrong_guess_count: i64,
    hanja_hints: Vec<HanjaHint>,
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
pub struct DayHistory {
    pub date: String,
    pub total: i64,
    pub correct: i64,
    pub percentage: f64,
}

#[derive(Serialize)]
pub struct ReviewHistoryResponse {
    pub days: Vec<DayHistory>,
}

#[derive(Serialize)]
pub struct StatsResponse {
    new_count: i64,
    due_count: i64,
    reviews_today: i64,
    correct_today: i64,
    percentage: Option<i64>,
    next_due_at: Option<String>,
    new_today_count: i64,
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
        "SELECT daily_new_card_limit, day_boundary_hour FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?;

    let daily_new_card_limit = user_row
        .as_ref()
        .and_then(|r| r.get::<Option<i64>, _>("daily_new_card_limit"))
        .unwrap_or(20);

    let day_boundary_hour = user_row
        .as_ref()
        .and_then(|r| r.get::<Option<i64>, _>("day_boundary_hour"))
        .unwrap_or(4);

    // Calculate the start of "today" based on day_boundary_hour (in local time)
    let now_local = Local::now();
    let current_hour = now_local.hour() as i64;
    let today_start_naive = if current_hour >= day_boundary_hour {
        // Today after boundary hour
        now_local.date_naive().and_hms_opt(day_boundary_hour as u32, 0, 0).unwrap()
    } else {
        // Before boundary hour, so "today" started yesterday
        (now_local.date_naive() - chrono::Days::new(1)).and_hms_opt(day_boundary_hour as u32, 0, 0).unwrap()
    };
    
    // Convert to UTC for database comparison
    let today_start_utc = Local
        .from_local_datetime(&today_start_naive)
        .single()
        .unwrap()
        .with_timezone(&Utc);
    
    // Format as SQLite datetime string (YYYY-MM-DD HH:MM:SS)
    let today_start_str = today_start_utc.format("%Y-%m-%d %H:%M:%S").to_string();

    // Count how many NEW cards the user has reviewed today
    // A "new" card is one where it's the user's first review (no prior review_history)
    // Check if new cards are suppressed (limit = 0) or if daily limit is reached
    let new_card_limit_reached = if daily_new_card_limit == 0 {
        true  // Suppress all new cards
    } else {
        // Count how many NEW cards the user has reviewed today
        let new_cards_today: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(DISTINCT rh.card_id)
            FROM review_history rh
            WHERE rh.user_id = ?
            AND rh.reviewed_at >= ?
            AND NOT EXISTS (
                SELECT 1 FROM review_history rh2
                WHERE rh2.user_id = rh.user_id
                AND rh2.card_id = rh.card_id
                AND rh2.reviewed_at < ?
            )
            "#
        )
        .bind(user_id)
        .bind(&today_start_str)
        .bind(&today_start_str)
        .fetch_one(&pool)
        .await?;

        // For prefetch requests (exclude param is set), use stricter limit to prevent race condition.
        // When the user is on card N (new card #19/20), the prefetch for N+1 should not return
        // a new card because by the time N+1 is displayed, card N will have been reviewed,
        // pushing the count to 20/20 and making N+1 display as 21/20.
        // For normal requests, use the actual limit.
        let is_prefetch = params.exclude.is_some();
        let threshold = if is_prefetch {
            daily_new_card_limit - 1  // Block at limit-1 for prefetch
        } else {
            daily_new_card_limit  // Block at limit for normal fetch
        };

        new_cards_today >= threshold
    };

    // Get next due card (prioritize due cards by due date, then new cards)
    // Exclude suspended cards via user_card_flags
    // Optionally skip a specific card_id (used for client-side prefetch)
    // When daily new card limit is 0 or reached (including limit-1 buffer), only show cards that have been reviewed before
    let exclude_id = params.exclude.unwrap_or(-1);
    
    let new_card_filter = if new_card_limit_reached {
        // If limit is 0 or reached, only show cards that have been reviewed before (have review history)
        "AND EXISTS (SELECT 1 FROM review_history WHERE card_id = c.id AND user_id = ?)"
    } else {
        ""
    };

    let query = format!(
        r#"
        SELECT
            c.id, c.word, c.definition, c.pos, c.origin_type, c.hanja, c.hanja_eum, c.grade,
            ct.trans_word, ct.trans_dfn,
            s.id as sentence_id, s.text as sentence, s.target,
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

    let mut query_builder = sqlx::query(&query)
        .bind(user_id)
        .bind(user_id)
        .bind(user_id);
    
    // Add extra bind for the new card filter if limit is reached
    if new_card_limit_reached {
        query_builder = query_builder.bind(user_id);
    }
    
    let row = query_builder
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
    let sentence_id: i64 = row.get("sentence_id");

    debug!("Selected card_id: {} ({})", card_id, word);

    // Fetch accepted alternative targets for this sentence
    let alternatives: Vec<String> = sqlx::query_scalar(
        "SELECT alt_target FROM sentence_alternative_targets WHERE sentence_id = ?"
    )
    .bind(sentence_id)
    .fetch_all(&pool)
    .await?;

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

    // Fetch hanja hints: hanja from other reviewed cards that share characters with this card
    let hanja_hints: Vec<HanjaHint> = if let Some(ref current_hanja) = hanja {
        if !current_hanja.is_empty() {
            let other_hanja_rows = sqlx::query(
                r#"
                SELECT DISTINCT c.hanja, c.hanja_eum
                FROM card_states cs
                INNER JOIN cards c ON c.id = cs.card_id
                WHERE cs.user_id = ?
                  AND cs.card_id != ?
                  AND c.hanja IS NOT NULL
                  AND c.hanja != ''
                "#
            )
            .bind(user_id)
            .bind(card_id)
            .fetch_all(&pool)
            .await?;

            let current_chars: std::collections::HashSet<char> =
                current_hanja.chars().collect();

            other_hanja_rows
                .iter()
                .filter_map(|row| {
                    let other_hanja: String = row.get("hanja");
                    let other_chars: std::collections::HashSet<char> =
                        other_hanja.chars().collect();
                    if current_chars.intersection(&other_chars).next().is_some() {
                        Some(HanjaHint {
                            hanja: other_hanja,
                            hanja_eum: row.get("hanja_eum"),
                        })
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            vec![]
        }
    } else {
        vec![]
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
            alternatives,
            speech_level,
            tense,
            difficulty,
            guess_count,
            wrong_guess_count,
            hanja_hints,
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

    // Get daily_new_card_limit setting (0 = suppress all new cards)
    let daily_new_card_limit: i64 = sqlx::query_scalar(
        "SELECT daily_new_card_limit FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .unwrap_or(20);

    // Get day_boundary_hour from user_settings (default to 4)
    let day_boundary_hour: i64 = sqlx::query_scalar(
        "SELECT day_boundary_hour FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .unwrap_or(4);

    // Calculate the "today" start timestamp (in local time)
    let now_local = Local::now();
    let today_start_naive = if now_local.hour() >= day_boundary_hour as u32 {
        now_local.date_naive().and_hms_opt(day_boundary_hour as u32, 0, 0).unwrap()
    } else {
        (now_local.date_naive() - chrono::Duration::days(1))
            .and_hms_opt(day_boundary_hour as u32, 0, 0)
            .unwrap()
    };
    
    // Convert to UTC for database comparison
    let today_start_utc = Local
        .from_local_datetime(&today_start_naive)
        .single()
        .unwrap()
        .with_timezone(&Utc);
    
    // Format as SQLite datetime string (YYYY-MM-DD HH:MM:SS)
    let today_start = today_start_utc.format("%Y-%m-%d %H:%M:%S").to_string();

    // Count new cards (cards not in card_states, excluding suspended)
    // If daily_new_card_limit is 0, new count is 0 (suppressed)
    let new_count_query = if daily_new_card_limit == 0 {
        // When new cards are suppressed (limit = 0), report 0 new cards
        r#"
        SELECT 0
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

    let new_count: i64 = if daily_new_card_limit == 0 {
        sqlx::query_scalar(new_count_query)
            .fetch_one(&pool)
            .await?
    } else {
        sqlx::query_scalar(new_count_query)
            .bind(user_id)
            .bind(user_id)
            .bind(user_id)
            .fetch_one(&pool)
            .await?
    };

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
        AND datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days') <= datetime('now')
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
        AND state != 'learning'
        AND datetime(reviewed_at) >= datetime(?)
        "#,
    )
    .bind(user_id)
    .bind(&today_start)
    .fetch_one(&pool)
    .await?;

    // Count correct reviews today (rating = 'good' or 'easy')
    let correct_today: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM review_history
        WHERE user_id = ?
        AND state != 'learning'
        AND rating IN ('good', 'easy')
        AND datetime(reviewed_at) >= datetime(?)
        "#,
    )
    .bind(user_id)
    .bind(&today_start)
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

    // Count how many NEW cards were reviewed today
    let new_today_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(DISTINCT rh.card_id)
        FROM review_history rh
        WHERE rh.user_id = ?
        AND rh.reviewed_at >= ?
        AND NOT EXISTS (
            SELECT 1 FROM review_history rh2
            WHERE rh2.user_id = rh.user_id
            AND rh2.card_id = rh.card_id
            AND rh2.reviewed_at < ?
        )
        "#
    )
    .bind(user_id)
    .bind(&today_start)
    .bind(&today_start)
    .fetch_one(&pool)
    .await?;

    Ok(Json(StatsResponse {
        new_count,
        due_count,
        reviews_today,
        correct_today,
        percentage,
        next_due_at,
        new_today_count,
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

pub async fn get_review_history(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<ReviewHistoryResponse>, AppError> {
    let user_id = auth.0;

    // Get day_boundary_hour from user_settings (default 4)
    let day_boundary_hour: i64 = sqlx::query_scalar(
        "SELECT day_boundary_hour FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .unwrap_or(4);

    // Subtracting day_boundary_hour from reviewed_at shifts timestamps so that
    // date() gives the correct "logical day" regardless of the boundary hour.
    // We look back far enough to cover 5 full boundary-aligned days.
    let lookback_hours = day_boundary_hour + 24 * 5;

    let rows = sqlx::query(
        r#"
        SELECT
            date(datetime(reviewed_at, printf('-%d hours', ?))) AS day,
            COUNT(*) AS total,
            SUM(CASE WHEN rating = 'good' THEN 1 ELSE 0 END) AS correct
        FROM review_history
        WHERE user_id = ?
          AND reviewed_at >= datetime('now', printf('-%d hours', ?))
        GROUP BY day
        ORDER BY day ASC
        LIMIT 5
        "#,
    )
    .bind(day_boundary_hour)
    .bind(user_id)
    .bind(lookback_hours)
    .fetch_all(&pool)
    .await?;

    let days = rows
        .iter()
        .map(|row| {
            let total: i64 = row.get("total");
            let correct: i64 = row.get("correct");
            let percentage = if total > 0 {
                (correct as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            DayHistory {
                date: row.get("day"),
                total,
                correct,
                percentage,
            }
        })
        .collect();

    Ok(Json(ReviewHistoryResponse { days }))
}