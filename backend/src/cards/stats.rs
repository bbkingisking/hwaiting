//! Aggregate views over `review_history`/`card_states`: the status-bar
//! summary, the rolling per-day history chart, the all-time summary with
//! streaks, and the by-POS/origin accuracy breakdown. None of these affect
//! scheduling; they all read what [`super::check::check_answer`] already wrote.

use axum::{extract::State, Json};
use chrono::{Local, Timelike};
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use utoipa::ToSchema;

use crate::error::AppError;

use super::time::{
    accuracy_percentage, logical_day_shift, logical_today_start, parse_flexible_datetime,
    sqlite_datetime, CORRECT_REVIEW_SQL, COUNTED_REVIEW_SQL,
};

#[derive(Serialize, ToSchema)]
pub struct StatsResponse {
    new_count: i64,
    due_count: i64,
    reviews_today: i64,
    correct_today: i64,
    percentage: Option<i64>,
    next_due_at: Option<String>,
    new_today_count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct DayHistory {
    pub date: String,
    pub total: i64,
    pub correct: i64,
    // Truncated integer, same computation as the status bar percentage
    pub percentage: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ReviewHistoryResponse {
    pub days: Vec<DayHistory>,
}

#[derive(Serialize, ToSchema)]
pub struct HistorySummary {
    pub total_reviews: i64,
    pub total_cards_reviewed: i64,
    pub cards_learning: i64,
    pub cards_review: i64,
    pub cards_relearning: i64,
    pub cards_unseen: i64,
    pub total_accuracy: f64,
    pub avg_reviews_per_day: f64,
    pub first_review_date: Option<String>,
    pub current_streak: i64,
    pub longest_streak: i64,
}

#[derive(Serialize, ToSchema)]
pub struct BreakdownRow {
    label: String,
    reviews: i64,
    correct: i64,
    accuracy: f64,
}

#[derive(Serialize, ToSchema)]
pub struct HistoryBreakdownResponse {
    by_pos: Vec<BreakdownRow>,
    by_origin: Vec<BreakdownRow>,
}

// Get statistics
#[utoipa::path(
    get,
    path = "/api/cards/stats",
    responses(
        (status = 200, description = "Status-bar summary stats", body = StatsResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
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

    // Start of the user's current logical day, as UTC for database comparison
    let today_start = sqlite_datetime(logical_today_start(day_boundary_hour));

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
    let reviews_today: i64 = sqlx::query_scalar(&format!(
        r#"
        SELECT COUNT(*)
        FROM review_history
        WHERE user_id = ?
        AND {COUNTED_REVIEW_SQL}
        AND datetime(reviewed_at) >= datetime(?)
        "#,
    ))
    .bind(user_id)
    .bind(&today_start)
    .fetch_one(&pool)
    .await?;

    // Count correct reviews today
    let correct_today: i64 = sqlx::query_scalar(&format!(
        r#"
        SELECT COUNT(*)
        FROM review_history
        WHERE user_id = ?
        AND {COUNTED_REVIEW_SQL}
        AND {CORRECT_REVIEW_SQL}
        AND datetime(reviewed_at) >= datetime(?)
        "#,
    ))
    .bind(user_id)
    .bind(&today_start)
    .fetch_one(&pool)
    .await?;

    let percentage = accuracy_percentage(correct_today, reviews_today);

    // Find when the next card becomes due
    let next_due_at: Option<String> = sqlx::query_scalar(
        r#"
        SELECT strftime('%Y-%m-%dT%H:%M:%SZ', MIN(datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days')))
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
    .fetch_one(&pool)
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

#[utoipa::path(
    get,
    path = "/api/cards/history",
    responses(
        (status = 200, description = "Per-day review history for a rolling window", body = ReviewHistoryResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
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

    // Same logical-day definition as get_stats, so today's bucket here matches
    // the status bar exactly. The window covers today plus the 4 days before.
    let day_shift = logical_day_shift(day_boundary_hour);
    let window_start = sqlite_datetime(
        logical_today_start(day_boundary_hour) - chrono::Duration::days(4),
    );

    let rows = sqlx::query(&format!(
        r#"
        SELECT
            date(datetime(reviewed_at, ?)) AS day,
            COUNT(*) AS total,
            SUM(CASE WHEN {CORRECT_REVIEW_SQL} THEN 1 ELSE 0 END) AS correct
        FROM review_history
        WHERE user_id = ?
          AND {COUNTED_REVIEW_SQL}
          AND datetime(reviewed_at) >= datetime(?)
        GROUP BY day
        ORDER BY day ASC
        "#,
    ))
    .bind(&day_shift)
    .bind(user_id)
    .bind(&window_start)
    .fetch_all(&pool)
    .await?;

    let days = rows
        .iter()
        .map(|row| {
            let total: i64 = row.get("total");
            let correct: i64 = row.get("correct");
            DayHistory {
                date: row.get("day"),
                total,
                correct,
                percentage: accuracy_percentage(correct, total).unwrap_or(0),
            }
        })
        .collect();

    Ok(Json(ReviewHistoryResponse { days }))
}

#[utoipa::path(
    get,
    path = "/api/cards/history-summary",
    responses(
        (status = 200, description = "Aggregate review history summary + streaks", body = HistorySummary),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn get_history_summary(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<HistorySummary>, AppError> {
    let user_id = auth.0;

    // Get day_boundary_hour from user_settings (default 4)
    let day_boundary_hour: i64 = sqlx::query_scalar(
        "SELECT day_boundary_hour FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .unwrap_or(4);

    // Query 1: Aggregate review stats. Accuracy only counts post-first-exposure
    // reviews (same rule as the status bar); the volume stats count everything.
    let stats_row = sqlx::query(&format!(
        r#"
        SELECT
            COUNT(*) AS total_reviews,
            COUNT(DISTINCT card_id) AS total_cards_reviewed,
            COALESCE(
                CAST(SUM(CASE WHEN {COUNTED_REVIEW_SQL} AND {CORRECT_REVIEW_SQL} THEN 1 ELSE 0 END) AS REAL)
                / NULLIF(SUM(CASE WHEN {COUNTED_REVIEW_SQL} THEN 1 ELSE 0 END), 0) * 100,
                0
            ) AS total_accuracy,
            MIN(reviewed_at) AS first_review_date,
            COUNT(DISTINCT date(reviewed_at)) AS distinct_days
        FROM review_history
        WHERE user_id = ?
        "#,
    ))
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    let total_reviews: i64 = stats_row.get("total_reviews");
    let total_cards_reviewed: i64 = stats_row.get("total_cards_reviewed");
    let total_accuracy: f64 = stats_row.get("total_accuracy");
    let distinct_days: i64 = stats_row.get("distinct_days");
    let avg_reviews_per_day = if distinct_days > 0 {
        total_reviews as f64 / distinct_days as f64
    } else {
        0.0
    };

    // Format first_review_date as YYYY-MM-DD
    let first_review_raw: Option<String> = stats_row.get("first_review_date");
    let first_review_date = first_review_raw.and_then(|s| {
        parse_flexible_datetime(&s)
            .ok()
            .map(|dt| dt.format("%Y-%m-%d").to_string())
    });

    // Query 2: Cards by current state
    let state_rows = sqlx::query(
        r#"
        SELECT state, COUNT(*) AS cnt
        FROM card_states
        WHERE user_id = ?
        GROUP BY state
        "#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let mut cards_learning: i64 = 0;
    let mut cards_review: i64 = 0;
    let mut cards_relearning: i64 = 0;
    for row in &state_rows {
        let state: String = row.get("state");
        let cnt: i64 = row.get("cnt");
        match state.as_str() {
            "learning" => cards_learning = cnt,
            "review" => cards_review = cnt,
            "relearning" => cards_relearning = cnt,
            _ => {}
        }
    }

    // Query 2b: Cards never reviewed by this user (same definition as the
    // status bar's new count, but ignoring the daily new card limit)
    let cards_unseen: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cards c
        LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        LEFT JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
        AND (cs.last_review IS NULL)
        AND (ucf.suspended IS NULL OR ucf.suspended = 0)
        "#,
    )
    .bind(user_id)
    .bind(user_id)
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    // Query 3: All review days (logical days) for streak calculation
    let day_rows = sqlx::query_scalar::<_, String>(
        r#"
        SELECT DISTINCT date(datetime(reviewed_at, ?)) AS day
        FROM review_history
        WHERE user_id = ?
        ORDER BY day ASC
        "#,
    )
    .bind(logical_day_shift(day_boundary_hour))
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    // Compute streaks in Rust
    let dates: Vec<chrono::NaiveDate> = day_rows
        .iter()
        .filter_map(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect();

    // Compute today in the user's boundary-adjusted timezone
    let now_local = Local::now();
    let today_boundary = if now_local.hour() as i64 >= day_boundary_hour {
        now_local.date_naive()
    } else {
        now_local.date_naive() - chrono::Days::new(1)
    };

    let current_streak = if dates.last() == Some(&today_boundary) {
        let mut streak = 1i64;
        for i in (0..dates.len() - 1).rev() {
            if dates[i + 1] - dates[i] == chrono::Duration::days(1) {
                streak += 1;
            } else {
                break;
            }
        }
        streak
    } else {
        0
    };

    let longest_streak = if dates.is_empty() {
        0
    } else {
        let mut max_streak = 1i64;
        let mut current = 1i64;
        for i in 1..dates.len() {
            if dates[i] - dates[i - 1] == chrono::Duration::days(1) {
                current += 1;
            } else {
                max_streak = max_streak.max(current);
                current = 1;
            }
        }
        max_streak.max(current)
    };

    Ok(Json(HistorySummary {
        total_reviews,
        total_cards_reviewed,
        cards_learning,
        cards_review,
        cards_relearning,
        cards_unseen,
        total_accuracy,
        avg_reviews_per_day,
        first_review_date,
        current_streak,
        longest_streak,
    }))
}

#[utoipa::path(
    get,
    path = "/api/cards/history-breakdown",
    responses(
        (status = 200, description = "Accuracy broken down by part-of-speech and origin type", body = HistoryBreakdownResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn get_history_breakdown(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<HistoryBreakdownResponse>, AppError> {
    let user_id = auth.0;

    // Breakdown by POS — only include rows where pos is not null/empty
    let pos_rows = sqlx::query(&format!(
        r#"
        SELECT
            pop.slug AS label,
            COUNT(*) AS reviews,
            SUM(CASE WHEN {CORRECT_REVIEW_SQL} THEN 1 ELSE 0 END) AS correct
        FROM review_history rh
        JOIN cards c ON c.id = rh.card_id
        JOIN parts_of_speech pop ON pop.id = c.pos_id
        WHERE rh.user_id = ?
          AND {COUNTED_REVIEW_SQL}
        GROUP BY pop.slug
        ORDER BY reviews DESC
        "#,
    ))
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let by_pos: Vec<BreakdownRow> = pos_rows
        .iter()
        .map(|row| {
            let reviews: i64 = row.get("reviews");
            let correct: i64 = row.get("correct");
            let accuracy = if reviews > 0 {
                (correct as f64 / reviews as f64) * 100.0
            } else {
                0.0
            };
            BreakdownRow {
                label: row.get("label"),
                reviews,
                correct,
                accuracy,
            }
        })
        .collect();

    // Breakdown by origin_type
    let origin_rows = sqlx::query(&format!(
        r#"
        SELECT
            ot.slug AS label,
            COUNT(*) AS reviews,
            SUM(CASE WHEN {CORRECT_REVIEW_SQL} THEN 1 ELSE 0 END) AS correct
        FROM review_history rh
        JOIN cards c ON c.id = rh.card_id
        JOIN origin_types ot ON ot.id = c.origin_type_id
        WHERE rh.user_id = ?
          AND {COUNTED_REVIEW_SQL}
        GROUP BY ot.slug
        ORDER BY reviews DESC
        "#,
    ))
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let by_origin: Vec<BreakdownRow> = origin_rows
        .iter()
        .map(|row| {
            let reviews: i64 = row.get("reviews");
            let correct: i64 = row.get("correct");
            let accuracy = if reviews > 0 {
                (correct as f64 / reviews as f64) * 100.0
            } else {
                0.0
            };
            BreakdownRow {
                label: row.get("label"),
                reviews,
                correct,
                accuracy,
            }
        })
        .collect();

    Ok(Json(HistoryBreakdownResponse { by_pos, by_origin }))
}
