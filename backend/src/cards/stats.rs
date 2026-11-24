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
    logical_today_date, review_day, sqlite_datetime,
};
use super::{ReviewPrefs, review_prefs};

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
pub struct TodayStatsResponse {
    /// Date of the current study day in the user's saved timezone.
    #[schema(format = Date)]
    study_day: String,
    /// Exclusive end of this study day, as a UTC timestamp.
    #[schema(format = DateTime)]
    day_ends_at: String,
    /// Answered reviews today, excluding first exposures.
    reviews_today: i64,
    correct_today: i64,
    /// Unrounded accuracy percentage (0–100); null when there are no reviews today.
    #[schema(required)]
    percentage: Option<f64>,
    /// Distinct cards first introduced today; first exposures are views, not reviews.
    new_today_count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct DayHistory {
    #[schema(format = Date)]
    pub date: String,
    pub total: i64,
    pub correct: i64,
    /// Unrounded accuracy percentage (0–100).
    #[schema(required)]
    pub percentage: Option<f64>,
}

#[derive(Serialize, ToSchema)]
pub struct HistoryResponse {
    /// Rolling 5-day (today + 4 back) per-day review counts, for the small history chart.
    pub timeseries: Vec<DayHistory>,
    /// All-time review aggregates and streaks; first exposures are excluded.
    pub summary: HistorySummary,
    /// All-time accuracy broken down by part-of-speech and by origin type.
    pub breakdown: HistoryBreakdownResponse,
}

#[derive(Serialize, ToSchema)]
pub struct HistorySummary {
    pub total_reviews: i64,
    pub total_cards_reviewed: i64,
    #[schema(required)]
    pub total_accuracy: Option<f64>,
    pub reviews_per_active_day: f64,
    #[schema(required)]
    #[schema(format = Date)]
    pub first_review_date: Option<String>,
    pub current_streak: i64,
    pub longest_streak: i64,
}

#[derive(Serialize, ToSchema)]
pub struct BreakdownRow<T> {
    slug: T,
    reviews: i64,
    correct: i64,
    accuracy: f64,
}

#[derive(Serialize, ToSchema)]
pub struct HistoryBreakdownResponse {
    by_pos: Vec<BreakdownRow<crate::classifications::PartOfSpeech>>,
    by_origin: Vec<BreakdownRow<crate::classifications::OriginType>>,
}

#[utoipa::path(
    get,
    path = "/api/stats/today",
    description = "Today's review totals, accuracy, and first-view count for the authenticated user. First exposures are views, not reviews. The study day follows the saved timezone and day-boundary hour; day_ends_at is its exclusive UTC end. No queue counts or scheduling information are included.",
    responses(
        (status = 200, description = "Current study-day activity", body = TodayStatsResponse),
        ),
    security(("cookie_auth" = [])),
    tag = "stats"
)]
pub async fn get_today_stats(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<TodayStatsResponse>, AppError> {
    let ReviewPrefs {
        day_boundary_hour,
        study_timezone,
        ..
    } = review_prefs(&pool, auth.0).await?;
    let zone = crate::user::parse_study_timezone(study_timezone.as_deref())?;
    let now = Utc::now().with_timezone(&zone);
    let start = logical_day_start(&now, day_boundary_hour, 0);
    let end = logical_day_start(&now, day_boundary_hour, 1);
    // One read keeps numerator, denominator, and first-view count consistent.
    // Normalize timestamps so SQLite and RFC3339 imports share the same bounds.
    let row = sqlx::query(&format!(
        r#"
        SELECT
            COALESCE(SUM(CASE WHEN {COUNTED_REVIEW_SQL} THEN 1 ELSE 0 END), 0) AS reviews,
            COALESCE(SUM(CASE WHEN {COUNTED_REVIEW_SQL} AND {CORRECT_REVIEW_SQL} THEN 1 ELSE 0 END), 0) AS correct,
            COUNT(DISTINCT CASE WHEN NOT EXISTS (
                SELECT 1 FROM review_history earlier
                WHERE earlier.user_id = rh.user_id AND earlier.card_id = rh.card_id
                  AND datetime(earlier.reviewed_at) < datetime(?)
            ) THEN rh.card_id END) AS new_cards
        FROM review_history rh
        WHERE user_id = ?
          AND datetime(reviewed_at) >= datetime(?)
          AND datetime(reviewed_at) < datetime(?)
        "#,
    ))
    .bind(sqlite_datetime(start))
    .bind(auth.0)
    .bind(sqlite_datetime(start))
    .bind(sqlite_datetime(end))
    .fetch_one(&pool)
    .await?;
    let reviews_today = row.get("reviews");
    let correct_today = row.get("correct");
    Ok(Json(TodayStatsResponse {
        study_day: logical_today_date(now.naive_local(), day_boundary_hour).to_string(),
        day_ends_at: end.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        reviews_today,
        correct_today,
        percentage: accuracy_percentage(correct_today, reviews_today),
        new_today_count: row.get("new_cards"),
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
    let today = logical_today_date(now.naive_local(), day_boundary_hour);
    for offset in 0..5 {
        days.entry(today - chrono::Duration::days(offset))
            .or_insert((0, 0));
    }
    Ok(days
        .into_iter()
        .map(|(day, (total, correct))| DayHistory {
            date: day.to_string(),
            total,
            correct,
            percentage: accuracy_percentage(correct, total),
        })
        .collect())
}

async fn query_review_dates<Tz: TimeZone>(
    pool: &SqlitePool,
    user_id: i64,
    day_boundary_hour: i64,
    zone: &Tz,
) -> Result<Vec<chrono::NaiveDate>, AppError> {
    let timestamps = sqlx::query_scalar::<_, String>(&format!(
        "SELECT DISTINCT reviewed_at FROM review_history WHERE user_id = ? AND {COUNTED_REVIEW_SQL}"
    ))
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
    // All reported review metrics exclude first exposures.
    let stats_row = sqlx::query(&format!(
        r#"
        SELECT
            COUNT(*) AS total_reviews,
            COUNT(DISTINCT card_id) AS total_cards_reviewed,
            CAST(SUM(CASE WHEN {CORRECT_REVIEW_SQL} THEN 1 ELSE 0 END) AS REAL)
                / NULLIF(COUNT(*), 0) * 100 AS total_accuracy
        FROM review_history
        WHERE user_id = ? AND {COUNTED_REVIEW_SQL}
        "#,
    ))
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    let total_reviews: i64 = stats_row.get("total_reviews");
    let total_cards_reviewed: i64 = stats_row.get("total_cards_reviewed");
    let total_accuracy: Option<f64> = stats_row.get("total_accuracy");
    let dates = query_review_dates(pool, user_id, day_boundary_hour, zone).await?;
    let reviews_per_active_day = if dates.is_empty() {
        0.0
    } else {
        total_reviews as f64 / dates.len() as f64
    };
    let first_review_date = dates.first().map(ToString::to_string);

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
        total_accuracy,
        reviews_per_active_day,
        first_review_date,
        current_streak,
        longest_streak,
    })
}

async fn query_breakdown(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<HistoryBreakdownResponse, AppError> {
    // Breakdown by POS — only include rows where pos is not null/empty
    let pos_rows = sqlx::query(&format!(
        r#"
        SELECT
            pop.slug AS slug,
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
    .fetch_all(pool)
    .await?;

    let by_pos: Vec<BreakdownRow<crate::classifications::PartOfSpeech>> = pos_rows
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
                slug: row.get("slug"),
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
            ot.slug AS slug,
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
    .fetch_all(pool)
    .await?;

    let by_origin: Vec<BreakdownRow<crate::classifications::OriginType>> = origin_rows
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
                slug: row.get("slug"),
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
    path = "/api/stats/history",
    description = "Return aggregated review history: daily counts, all-time totals and streaks, and accuracy by part of speech and word origin. Daily groupings use the user's study timezone and day-boundary hour; individual review records are available through data export.",
    responses(
        (status = 200, description = "Aggregated history: daily counts, all-time summary and streaks, and POS/origin accuracy; not individual review records", body = HistoryResponse),
        ),
    security(("cookie_auth" = [])),
    tag = "stats"
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

#[cfg(test)]
#[path = "../tests/cards/today.rs"]
mod today_tests;
