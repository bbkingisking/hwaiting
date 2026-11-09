//! Aggregate views over `review_history`/`cards_states`: the status-bar
//! summary, the rolling per-day history chart, the all-time summary with
//! streaks, and the by-POS/origin accuracy breakdown. None of these affect
//! scheduling; they all read what [`super::check::check_answer`] already wrote.

use axum::{Json, extract::State};
use chrono::{TimeZone, Utc};
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use std::collections::{BTreeMap, BTreeSet};
use utoipa::ToSchema;

use crate::error::AppError;

use super::time::{
    CORRECT_REVIEW_SQL, COUNTED_REVIEW_SQL, accuracy_percentage, logical_day_start,
    logical_today_date, logical_today_start, parse_flexible_datetime, review_day, sqlite_datetime,
};
use super::{MASTERED_STATE, ReviewPrefs, review_prefs};

/// Current-streak and longest-streak, from the sorted, deduplicated list of
/// logical days the user reviewed on and today's own logical date. Split out
/// from `query_summary` so the streak arithmetic - the actual logic, as
/// opposed to the SQL that produces `dates` - can be tested without a
/// database. `dates` must be sorted ascending and duplicate-free, as
/// guaranteed by `query_review_dates`.
fn compute_streaks(dates: &[chrono::NaiveDate], today: chrono::NaiveDate) -> (i64, i64) {
    let current_streak = if dates.last() == Some(&today) {
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

    (current_streak, longest_streak)
}

#[derive(Serialize, ToSchema)]
pub struct StatsResponse {
    new_count: i64,
    due_count: i64,
    reviews_today: i64,
    correct_today: i64,
    /// Unrounded accuracy percentage (0–100); null when there are no counted reviews today.
    percentage: Option<f64>,
    next_due_at: Option<String>,
    new_today_count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct DayHistory {
    pub date: String,
    pub total: i64,
    pub correct: i64,
    /// Unrounded accuracy percentage (0–100).
    pub percentage: f64,
}

#[derive(Serialize, ToSchema)]
pub struct HistoryResponse {
    /// Rolling 5-day (today + 4 back) per-day review counts, for the small history chart.
    pub timeseries: Vec<DayHistory>,
    /// All-time aggregate + current FSRS state distribution + streaks.
    pub summary: HistorySummary,
    /// All-time accuracy broken down by part-of-speech and by origin type.
    pub breakdown: HistoryBreakdownResponse,
}

#[derive(Serialize, ToSchema)]
pub struct HistorySummary {
    pub total_reviews: i64,
    pub total_cards_reviewed: i64,
    pub cards_learning: i64,
    pub cards_review: i64,
    pub cards_relearning: i64,
    pub cards_unseen: i64,
    pub cards_suppressed: i64,
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
    security(("cookie_auth" = [])),
    tag = "cards"
)]
pub async fn get_stats(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<StatsResponse>, AppError> {
    let user_id = auth.0;

    // daily_new_card_limit: 0 = suppress all new cards.
    let ReviewPrefs {
        day_boundary_hour,
        study_timezone,
        daily_new_card_limit,
        ..
    } = review_prefs(&pool, user_id).await?;

    // Start of the user's current logical day, as UTC for database comparison
    let today_start = sqlite_datetime(logical_today_start(
        day_boundary_hour,
        &crate::user::parse_study_timezone(study_timezone.as_deref())?,
    ));

    // Count new cards (cards not in cards_states, excluding suppressed)
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
        LEFT JOIN cards_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN users_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (cs.last_review IS NULL)
        AND (ucf.suppressed IS NULL OR ucf.suppressed = 0)
        "#
    };

    let new_count: i64 = if daily_new_card_limit == 0 {
        sqlx::query_scalar(new_count_query).fetch_one(&pool).await?
    } else {
        sqlx::query_scalar(new_count_query)
            .bind(user_id)
            .bind(user_id)
            .fetch_one(&pool)
            .await?
    };

    // Count due cards (existing cards with last_review set, excluding suppressed)
    let due_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cards c
        INNER JOIN cards_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN users_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE cs.last_review IS NOT NULL
        AND (ucf.suppressed IS NULL OR ucf.suppressed = 0)
        AND cs.due_at <= unixepoch('now')
        "#,
    )
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
        SELECT strftime('%Y-%m-%dT%H:%M:%SZ', MIN(cs.due_at), 'unixepoch')
        FROM cards c
        INNER JOIN cards_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN users_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE cs.due_at > unixepoch('now')
        AND (ucf.suppressed IS NULL OR ucf.suppressed = 0)
        "#,
    )
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
        "#,
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

async fn query_timeseries(
    pool: &SqlitePool,
    user_id: i64,
    day_boundary_hour: i64,
    zone: &chrono_tz::Tz,
) -> Result<Vec<DayHistory>, AppError> {
    query_timeseries_at(
        pool,
        user_id,
        day_boundary_hour,
        &Utc::now().with_timezone(zone),
    )
    .await
}

async fn query_timeseries_at<Tz: TimeZone>(
    pool: &SqlitePool,
    user_id: i64,
    day_boundary_hour: i64,
    now: &chrono::DateTime<Tz>,
) -> Result<Vec<DayHistory>, AppError> {
    let window_start = sqlite_datetime(logical_day_start(now, day_boundary_hour, -4));
    let rows = sqlx::query(&format!(
        r#"
        SELECT reviewed_at, COUNT(*) AS total,
            SUM(CASE WHEN {CORRECT_REVIEW_SQL} THEN 1 ELSE 0 END) AS correct
        FROM review_history
        WHERE user_id = ?
          AND {COUNTED_REVIEW_SQL}
          AND datetime(reviewed_at) >= datetime(?)
        GROUP BY reviewed_at
        "#,
    ))
    .bind(user_id)
    .bind(&window_start)
    .fetch_all(pool)
    .await?;

    let mut days = BTreeMap::new();
    for row in rows {
        let day = review_day(row.get("reviewed_at"), day_boundary_hour, &now.timezone())
            .map_err(|e| AppError::Internal(format!("Invalid review timestamp: {e}")))?;
        let counts = days.entry(day).or_insert((0i64, 0i64));
        counts.0 += row.get::<i64, _>("total");
        counts.1 += row.get::<i64, _>("correct");
    }
    Ok(days
        .into_iter()
        .map(|(day, (total, correct))| DayHistory {
            date: day.to_string(),
            total,
            correct,
            percentage: accuracy_percentage(correct, total).unwrap_or(0.0),
        })
        .collect())
}

async fn query_review_dates<Tz: TimeZone>(
    pool: &SqlitePool,
    user_id: i64,
    day_boundary_hour: i64,
    zone: &Tz,
) -> Result<Vec<chrono::NaiveDate>, AppError> {
    let timestamps = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT reviewed_at FROM review_history WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    let mut dates = BTreeSet::new();
    for timestamp in timestamps {
        dates.insert(
            review_day(&timestamp, day_boundary_hour, zone)
                .map_err(|e| AppError::Internal(format!("Invalid review timestamp: {e}")))?,
        );
    }
    Ok(dates.into_iter().collect())
}

async fn query_summary(
    pool: &SqlitePool,
    user_id: i64,
    day_boundary_hour: i64,
    zone: &chrono_tz::Tz,
) -> Result<HistorySummary, AppError> {
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
                0.0
            ) AS total_accuracy,
            MIN(reviewed_at) AS first_review_date,
            COUNT(DISTINCT date(reviewed_at)) AS distinct_days
        FROM review_history
        WHERE user_id = ?
        "#,
    ))
    .bind(user_id)
    .fetch_one(pool)
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
        FROM cards_states
        WHERE user_id = ?
        GROUP BY state
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let mut cards_learning: i64 = 0;
    let mut cards_review: i64 = 0;
    let mut cards_relearning: i64 = 0;
    for row in &state_rows {
        let state: String = row.get("state");
        let cnt: i64 = row.get("cnt");
        match state.as_str() {
            "learning" => cards_learning = cnt,
            MASTERED_STATE => cards_review = cnt,
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
        LEFT JOIN cards_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN users_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (cs.last_review IS NULL)
        AND (ucf.suppressed IS NULL OR ucf.suppressed = 0)
        "#,
    )
    .bind(user_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    let cards_suppressed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users_card_flags WHERE user_id = ? AND suppressed = 1",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    let dates = query_review_dates(pool, user_id, day_boundary_hour, zone).await?;

    // Compute today in the user's boundary-adjusted timezone - the same
    // definition `logical_today_start` uses for the rest of the app, so a
    // review logged at 3am with a 4am boundary counts toward yesterday's
    // streak day here too.
    let today_boundary = logical_today_date(
        Utc::now().with_timezone(zone).naive_local(),
        day_boundary_hour,
    );

    let (current_streak, longest_streak) = compute_streaks(&dates, today_boundary);

    Ok(HistorySummary {
        total_reviews,
        total_cards_reviewed,
        cards_learning,
        cards_review,
        cards_relearning,
        cards_unseen,
        cards_suppressed,
        total_accuracy,
        avg_reviews_per_day,
        first_review_date,
        current_streak,
        longest_streak,
    })
}

async fn query_breakdown(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<HistoryBreakdownResponse, AppError> {
    // pop.slug/ot.slug are ascii identifiers as of migration 20240101000043
    // (the human-readable text moved to parts_of_speech_labels/
    // origin_types_labels), so the breakdown's `label` needs the same
    // `_labels` join `field_values::fetch_field_values` uses, or this chart
    // would start rendering `verb`/`native-korean` instead of readable text.
    let eng_id = crate::enum_lookup::eng_language_id(pool).await?;

    // Breakdown by POS — only include rows where pos is not null/empty
    let pos_rows = sqlx::query(&format!(
        r#"
        SELECT
            popl.label AS label,
            COUNT(*) AS reviews,
            SUM(CASE WHEN {CORRECT_REVIEW_SQL} THEN 1 ELSE 0 END) AS correct
        FROM review_history rh
        JOIN cards c ON c.id = rh.card_id
        JOIN parts_of_speech pop ON pop.id = c.pos_id
        JOIN parts_of_speech_labels popl ON popl.pos_id = pop.id AND popl.language_id = ?
        WHERE rh.user_id = ?
          AND {COUNTED_REVIEW_SQL}
        GROUP BY popl.label
        ORDER BY reviews DESC
        "#,
    ))
    .bind(eng_id)
    .bind(user_id)
    .fetch_all(pool)
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
            otl.label AS label,
            COUNT(*) AS reviews,
            SUM(CASE WHEN {CORRECT_REVIEW_SQL} THEN 1 ELSE 0 END) AS correct
        FROM review_history rh
        JOIN cards c ON c.id = rh.card_id
        JOIN origin_types ot ON ot.id = c.origin_type_id
        JOIN origin_types_labels otl ON otl.origin_type_id = ot.id AND otl.language_id = ?
        WHERE rh.user_id = ?
          AND {COUNTED_REVIEW_SQL}
        GROUP BY otl.label
        ORDER BY reviews DESC
        "#,
    ))
    .bind(eng_id)
    .bind(user_id)
    .fetch_all(pool)
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

    Ok(HistoryBreakdownResponse { by_pos, by_origin })
}

#[utoipa::path(
    get,
    path = "/api/cards/history",
    responses(
        (status = 200, description = "Time series, all-time summary + streaks, and POS/origin accuracy breakdown, run concurrently", body = HistoryResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "cards"
)]
pub async fn get_history(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<HistoryResponse>, AppError> {
    let user_id = auth.0;

    // Fetched once here rather than by query_timeseries/query_summary each -
    // both need the same day_boundary_hour, and re-fetching it a second time
    // for the same request bought nothing.
    let ReviewPrefs {
        day_boundary_hour,
        study_timezone,
        ..
    } = review_prefs(&pool, user_id).await?;

    let zone = crate::user::parse_study_timezone(study_timezone.as_deref())?;
    let (timeseries, summary, breakdown) = tokio::try_join!(
        query_timeseries(&pool, user_id, day_boundary_hour, &zone),
        query_summary(&pool, user_id, day_boundary_hour, &zone),
        query_breakdown(&pool, user_id),
    )?;

    Ok(Json(HistoryResponse {
        timeseries,
        summary,
        breakdown,
    }))
}

#[cfg(test)]
#[path = "../tests/cards/stats.rs"]
mod tests;
