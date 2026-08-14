//! Logical-day and accuracy helpers shared by [`super::next`] (new-card
//! gating) and [`super::stats`] (status bar, history, streaks) - the review
//! flow and the stats surfaces must agree on what "today" and "accuracy"
//! mean, so both go through these instead of computing their own.

use chrono::{Local, TimeZone, Timelike, Utc};

// Accuracy stats must agree everywhere they are shown (status bar, review
// history chart, summary, breakdowns). These fragments define which
// review_history rows count: a card's first-ever review is recorded with
// state = 'learning' and is excluded from accuracy either way.
pub(super) const COUNTED_REVIEW_SQL: &str = "state != 'learning'";
pub(super) const CORRECT_REVIEW_SQL: &str = "rating IN ('good', 'easy')";

/// Start of the user's current logical day: `day_boundary_hour` o'clock local
/// time, today if that moment has passed, otherwise yesterday.
pub(super) fn logical_today_start(day_boundary_hour: i64) -> chrono::DateTime<Utc> {
    let now_local = Local::now();
    let today_start_naive = if now_local.hour() >= day_boundary_hour as u32 {
        now_local.date_naive().and_hms_opt(day_boundary_hour as u32, 0, 0).unwrap()
    } else {
        (now_local.date_naive() - chrono::Duration::days(1))
            .and_hms_opt(day_boundary_hour as u32, 0, 0)
            .unwrap()
    };
    Local
        .from_local_datetime(&today_start_naive)
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

/// Format a UTC datetime for comparison against SQLite datetime() values.
pub(super) fn sqlite_datetime(dt: chrono::DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// SQLite datetime modifier that shifts a UTC `reviewed_at` so that date()
/// yields its logical day — the same boundary as `logical_today_start`.
pub(super) fn logical_day_shift(day_boundary_hour: i64) -> String {
    let utc_offset_minutes = i64::from(Local::now().offset().local_minus_utc()) / 60;
    format!("{:+} minutes", utc_offset_minutes - day_boundary_hour * 60)
}

/// Truncated integer accuracy percentage; None when there are no reviews.
pub(super) fn accuracy_percentage(correct: i64, total: i64) -> Option<i64> {
    (total > 0).then(|| correct * 100 / total)
}
