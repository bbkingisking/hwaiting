//! Aggregate views over `review_history`/`cards_states`: the status-bar
//! summary, the rolling per-day history chart, the all-time summary with
//! streaks, and the by-POS/origin accuracy breakdown. None of these affect
//! scheduling; they all read what [`super::check::check_answer`] already wrote.

use axum::{extract::State, Json};
use chrono::Local;
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use utoipa::ToSchema;

use crate::error::AppError;

use super::time::{
    accuracy_percentage, logical_day_shift, logical_today_date, logical_today_start,
    parse_flexible_datetime, sqlite_datetime, CORRECT_REVIEW_SQL, COUNTED_REVIEW_SQL,
};
use super::{review_prefs, ReviewPrefs, MASTERED_STATE};

/// Current-streak and longest-streak, from the sorted, deduplicated list of
/// logical days the user reviewed on and today's own logical date. Split out
/// from `query_summary` so the streak arithmetic - the actual logic, as
/// opposed to the SQL that produces `dates` - can be tested without a
/// database. `dates` must be sorted ascending and duplicate-free, which is
/// what `SELECT DISTINCT ... ORDER BY day ASC` already guarantees the one
/// caller.
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

    // daily_new_card_limit: 0 = suppress all new cards.
    let ReviewPrefs { day_boundary_hour, daily_new_card_limit, .. } = review_prefs(&pool, user_id).await?;

    // Start of the user's current logical day, as UTC for database comparison
    let today_start = sqlite_datetime(logical_today_start(day_boundary_hour));

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
        sqlx::query_scalar(new_count_query)
            .fetch_one(&pool)
            .await?
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
        AND datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days') <= datetime('now')
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
        SELECT strftime('%Y-%m-%dT%H:%M:%SZ', MIN(datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days')))
        FROM cards c
        INNER JOIN cards_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN users_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days') > datetime('now')
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

async fn query_timeseries(pool: &SqlitePool, user_id: i64, day_boundary_hour: i64) -> Result<Vec<DayHistory>, AppError> {
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
    .fetch_all(pool)
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

    Ok(days)
}

async fn query_summary(pool: &SqlitePool, user_id: i64, day_boundary_hour: i64) -> Result<HistorySummary, AppError> {
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
    .fetch_all(pool)
    .await?;

    // Compute streaks in Rust
    let dates: Vec<chrono::NaiveDate> = day_rows
        .iter()
        .filter_map(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect();

    // Compute today in the user's boundary-adjusted timezone - the same
    // definition `logical_today_start` uses for the rest of the app, so a
    // review logged at 3am with a 4am boundary counts toward yesterday's
    // streak day here too.
    let today_boundary = logical_today_date(Local::now().naive_local(), day_boundary_hour);

    let (current_streak, longest_streak) = compute_streaks(&dates, today_boundary);

    Ok(HistorySummary {
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
    })
}

async fn query_breakdown(pool: &SqlitePool, user_id: i64) -> Result<HistoryBreakdownResponse, AppError> {
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
    security(("bearer_auth" = [])),
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
    let ReviewPrefs { day_boundary_hour, .. } = review_prefs(&pool, user_id).await?;

    let (timeseries, summary, breakdown) = tokio::try_join!(
        query_timeseries(&pool, user_id, day_boundary_hour),
        query_summary(&pool, user_id, day_boundary_hour),
        query_breakdown(&pool, user_id),
    )?;

    Ok(Json(HistoryResponse {
        timeseries,
        summary,
        breakdown,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn no_reviews_ever_is_no_streak() {
        assert_eq!(compute_streaks(&[], d("2026-03-15")), (0, 0));
    }

    #[test]
    fn single_review_today_is_a_streak_of_one() {
        let dates = [d("2026-03-15")];
        assert_eq!(compute_streaks(&dates, d("2026-03-15")), (1, 1));
    }

    #[test]
    fn consecutive_run_ending_today() {
        let dates = [d("2026-03-13"), d("2026-03-14"), d("2026-03-15")];
        assert_eq!(compute_streaks(&dates, d("2026-03-15")), (3, 3));
    }

    // Reviewed every day through yesterday, but hasn't reviewed yet today.
    // `current_streak` reports 0 here (the streak is only "alive" once
    // today's own review lands), which is the existing, intentional
    // behavior - documented by this test so a future change to it is a
    // deliberate decision rather than an accidental regression.
    #[test]
    fn run_ending_yesterday_reports_zero_current_streak() {
        let dates = [d("2026-03-13"), d("2026-03-14")];
        let (current, longest) = compute_streaks(&dates, d("2026-03-15"));
        assert_eq!(current, 0);
        assert_eq!(longest, 2);
    }

    #[test]
    fn gap_breaks_the_current_streak_count() {
        // Reviewed 3/10, then nothing until 3/14-3/15 (today).
        let dates = [d("2026-03-10"), d("2026-03-14"), d("2026-03-15")];
        assert_eq!(compute_streaks(&dates, d("2026-03-15")), (2, 2));
    }

    #[test]
    fn longest_streak_can_exceed_current_streak() {
        // A long run in the past, then a short one ending today.
        let dates = [
            d("2026-03-01"),
            d("2026-03-02"),
            d("2026-03-03"),
            d("2026-03-04"),
            d("2026-03-05"),
            d("2026-03-14"),
            d("2026-03-15"),
        ];
        assert_eq!(compute_streaks(&dates, d("2026-03-15")), (2, 5));
    }

    // No test for an all-gaps history (every day isolated, longest == 1):
    // it exercises the exact same "non-consecutive resets the run" branch
    // gap_breaks_the_current_streak_count already covers.

    // --- get_stats / get_history (handler-level, in-process SQLite) ---------

    use crate::test_support::{test_pool, test_user};

    async fn insert_review(pool: &SqlitePool, user_id: i64, card_id: i64, rating: &str, state: &str, reviewed_at: &str) {
        sqlx::query(
            "INSERT INTO review_history (user_id, card_id, rating, reviewed_at, state) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(card_id)
        .bind(rating)
        .bind(reviewed_at)
        .bind(state)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn reviews_before_the_day_boundary_dont_count_as_today() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;

        // Same boundary the handler will compute (no users_settings row -> default hour 4).
        let boundary = logical_today_start(4);
        let before = sqlite_datetime(boundary - chrono::Duration::hours(1));
        let after = sqlite_datetime(boundary + chrono::Duration::hours(1));

        insert_review(&pool, user_id, 1, "good", "review", &before).await;
        insert_review(&pool, user_id, 2, "good", "review", &after).await;

        let stats = get_stats(State(pool.clone()), crate::auth::AuthUser(user_id))
            .await
            .unwrap()
            .0;

        assert_eq!(stats.reviews_today, 1);
        assert_eq!(stats.correct_today, 1);
    }

    #[tokio::test]
    async fn a_learning_first_review_is_excluded_from_todays_accuracy() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        let boundary = logical_today_start(4);
        let today = sqlite_datetime(boundary + chrono::Duration::hours(1));

        // A card's first-ever review is always logged as "learning" -
        // COUNTED_REVIEW_SQL excludes it from both the numerator and
        // denominator of today's accuracy.
        insert_review(&pool, user_id, 1, "good", "learning", &today).await;

        let stats = get_stats(State(pool.clone()), crate::auth::AuthUser(user_id))
            .await
            .unwrap()
            .0;
        assert_eq!(stats.reviews_today, 0);
        assert_eq!(stats.correct_today, 0);
        assert_eq!(stats.percentage, None);

        // A second, post-learning review of a different card does count.
        insert_review(&pool, user_id, 2, "again", "review", &today).await;
        let stats = get_stats(State(pool.clone()), crate::auth::AuthUser(user_id))
            .await
            .unwrap()
            .0;
        assert_eq!(stats.reviews_today, 1);
        assert_eq!(stats.correct_today, 0);
        assert_eq!(stats.percentage, Some(0));
    }

    #[tokio::test]
    async fn history_does_not_panic_for_a_user_with_cards_states_but_no_review_history() {
        // The exact shape of the fixed "panic in /api/cards/history for
        // users with no graded reviews" bug: a cards_states row with no
        // matching review_history at all (e.g. from a restored backup, or
        // state seeded directly).
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        sqlx::query(
            "INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state) \
             VALUES (?, 1, 5, 5, datetime('now'), 'review')",
        )
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

        let history = get_history(State(pool.clone()), crate::auth::AuthUser(user_id))
            .await
            .unwrap()
            .0;

        assert_eq!(history.summary.total_reviews, 0);
        assert_eq!(history.summary.total_accuracy, 0.0);
    }
}
