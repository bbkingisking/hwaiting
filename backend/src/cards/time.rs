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

/// Parses a timestamp that may be in either format this app has ever
/// written to a `TEXT` "datetime" column: RFC3339 (`DateTime::to_rfc3339()`,
/// what `check::check_answer` writes to `card_states.last_review`), or
/// SQLite's own `datetime('now')` format - what every column relying on a
/// schema `DEFAULT` instead of an explicit bind gets (e.g.
/// `review_history.reviewed_at`), and what `card_states.last_review` itself
/// falls back to after a backup restore that recreated the row via raw SQL
/// rather than through this application.
///
/// Three call sites (`check::check_answer`, `fsrs_admin::optimize_fsrs`,
/// `stats::get_history_summary`) used to each hand-roll their own subset of
/// this fallback chain, in a different order and without the initial
/// `parse_from_rfc3339` attempt this function leads with - so which formats
/// a given caller actually tolerated (and whether an RFC3339 timestamp with
/// a non-UTC offset would even survive it) wasn't answerable by reading any
/// one of them in isolation.
pub(super) fn parse_flexible_datetime(s: &str) -> Result<chrono::DateTime<Utc>, chrono::ParseError> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f"))
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .map(|ndt| chrono::DateTime::from_naive_utc_and_offset(ndt, Utc))
}
