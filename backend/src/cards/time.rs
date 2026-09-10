//! Logical-day and accuracy helpers shared by [`super::next`] (new-card
//! gating) and [`super::stats`] (status bar, history, streaks) - the review
//! flow and the stats surfaces must agree on what "today" and "accuracy"
//! mean, so both go through these instead of computing their own.

use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};

// Accuracy stats must agree everywhere they are shown (status bar, review
// history chart, summary, breakdowns). These fragments define which
// review_history rows count: a card's first-ever review is recorded with
// state = 'learning' and is excluded from accuracy either way.
pub(super) const COUNTED_REVIEW_SQL: &str = "state != 'learning'";
pub(super) const CORRECT_REVIEW_SQL: &str = "rating IN ('good', 'easy')";

/// The naive (zone-less) wall-clock instant `day_boundary_hour` o'clock
/// today-or-yesterday resolves to, given the caller's own idea of "now" as a
/// naive local timestamp. Split out from [`logical_today_start`] so the
/// boundary arithmetic itself - the part every past bug in this area
/// actually broke - can be tested without needing to fake the system
/// timezone: this half takes `now_local` as a plain value instead of calling
/// `Local::now()` itself.
fn logical_day_start_naive(now_local: NaiveDateTime, day_boundary_hour: i64) -> NaiveDateTime {
    if now_local.hour() >= day_boundary_hour as u32 {
        now_local.date().and_hms_opt(day_boundary_hour as u32, 0, 0).unwrap()
    } else {
        (now_local.date() - chrono::Duration::days(1))
            .and_hms_opt(day_boundary_hour as u32, 0, 0)
            .unwrap()
    }
}

/// Resolves a naive local wall-clock time to a UTC instant, without
/// panicking on the two cases a fixed UTC offset can't represent: a
/// spring-forward gap (the wall-clock moment never happened) or a
/// fall-back overlap (it happened twice). `logical_today_start` used to
/// call `.single().unwrap()` directly, which panics on both - taking down
/// every request that touches stats or scheduling, for every user, twice a
/// year on any host whose local zone observes DST at the boundary hour.
/// Ambiguous resolves to the earlier of the two instants (arbitrary but
/// deterministic); a gap is walked forward hour by hour to the first
/// wall-clock time that does exist, mirroring how a real clock crossing
/// the gap behaves.
fn resolve_local(naive: NaiveDateTime) -> chrono::DateTime<Utc> {
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),
        chrono::LocalResult::Ambiguous(earlier, _later) => earlier.with_timezone(&Utc),
        chrono::LocalResult::None => {
            for hours in 1..=4 {
                let shifted = naive + chrono::Duration::hours(hours);
                if let chrono::LocalResult::Single(dt) = Local.from_local_datetime(&shifted) {
                    return dt.with_timezone(&Utc);
                }
            }
            // No real timezone's DST gap is this wide; fall back to treating
            // the naive value as UTC rather than panic over a wall-clock
            // that (per the platform's own tz data) never existed.
            Utc.from_utc_datetime(&naive)
        }
    }
}

/// Start of the user's current logical day: `day_boundary_hour` o'clock local
/// time, today if that moment has passed, otherwise yesterday.
pub(super) fn logical_today_start(day_boundary_hour: i64) -> chrono::DateTime<Utc> {
    resolve_local(logical_day_start_naive(Local::now().naive_local(), day_boundary_hour))
}

/// The user's current logical *date* (no time-of-day component): the date
/// half of [`logical_day_start_naive`]. `stats::query_summary`'s streak
/// calculation needs exactly this - "is the most recent reviewed day the
/// same logical day as right now" - without the UTC conversion
/// `logical_today_start` does for the rest of the boundary, since streaks
/// compare against `date()`-truncated SQLite output rather than a UTC
/// instant. Sharing this with `logical_day_start_naive` rather than letting
/// the streak code re-derive "today, or yesterday before the boundary hour"
/// on its own is what keeps the two from silently disagreeing about which
/// day a 3am review lands on.
pub(super) fn logical_today_date(now_local: NaiveDateTime, day_boundary_hour: i64) -> NaiveDate {
    logical_day_start_naive(now_local, day_boundary_hour).date()
}

/// Format a UTC datetime for comparison against SQLite datetime() values.
pub(super) fn sqlite_datetime(dt: chrono::DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// The `{:+} minutes` arithmetic behind [`logical_day_shift`], taking the
/// caller's UTC offset directly instead of reading it from `Local::now()` -
/// split out for the same testability reason as `logical_day_start_naive`.
fn logical_day_shift_for_offset(utc_offset_minutes: i64, day_boundary_hour: i64) -> String {
    format!("{:+} minutes", utc_offset_minutes - day_boundary_hour * 60)
}

/// SQLite datetime modifier that shifts a UTC `reviewed_at` so that date()
/// yields its logical day — the same boundary as `logical_today_start`.
pub(super) fn logical_day_shift(day_boundary_hour: i64) -> String {
    let utc_offset_minutes = i64::from(Local::now().offset().local_minus_utc()) / 60;
    logical_day_shift_for_offset(utc_offset_minutes, day_boundary_hour)
}

/// Truncated integer accuracy percentage; None when there are no reviews.
pub(super) fn accuracy_percentage(correct: i64, total: i64) -> Option<i64> {
    (total > 0).then(|| correct * 100 / total)
}

/// Parses a timestamp that may be in either format this app has ever
/// written to a `TEXT` "datetime" column: RFC3339 (`DateTime::to_rfc3339()`,
/// what `check::check_answer` writes to `cards_states.last_review`), or
/// SQLite's own `datetime('now')` format - what every column relying on a
/// schema `DEFAULT` instead of an explicit bind gets (e.g.
/// `review_history.reviewed_at`), and what `cards_states.last_review` itself
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn ndt(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_hms_opt(h, mi, s).unwrap()
    }

    // --- logical_day_start_naive -------------------------------------------

    #[test]
    fn day_start_before_boundary_rolls_back_to_yesterday() {
        // 3:59am with a 4am boundary is still "yesterday" for review-day
        // purposes - a user grinding reviews past midnight shouldn't have
        // their day roll over at midnight.
        let now = ndt(2026, 3, 15, 3, 59, 0);
        assert_eq!(logical_day_start_naive(now, 4), ndt(2026, 3, 14, 4, 0, 0));
    }

    #[test]
    fn day_start_at_boundary_is_today() {
        let now = ndt(2026, 3, 15, 4, 0, 0);
        assert_eq!(logical_day_start_naive(now, 4), ndt(2026, 3, 15, 4, 0, 0));
    }

    #[test]
    fn day_start_after_boundary_is_today() {
        let now = ndt(2026, 3, 15, 23, 59, 59);
        assert_eq!(logical_day_start_naive(now, 4), ndt(2026, 3, 15, 4, 0, 0));
    }

    #[test]
    fn day_start_boundary_hour_zero_is_plain_midnight() {
        let now = ndt(2026, 3, 15, 0, 0, 0);
        assert_eq!(logical_day_start_naive(now, 0), ndt(2026, 3, 15, 0, 0, 0));
    }

    #[test]
    fn day_start_crosses_a_month_boundary() {
        let now = ndt(2026, 3, 1, 1, 0, 0);
        assert_eq!(logical_day_start_naive(now, 4), ndt(2026, 2, 28, 4, 0, 0));
    }

    // resolve_local's gap/ambiguous-instant handling (the actual DST-panic
    // fix) isn't covered by a test: exercising it needs a wall-clock time
    // that's genuinely a gap or overlap in the *test runner's own* local
    // zone, which isn't something a portable unit test can assume or fake
    // without pulling in a timezone-injection dependency. The unambiguous
    // case is implicitly covered by every logical_today_start caller in the
    // handler-level tests elsewhere in this crate.

    // --- logical_day_shift_for_offset ----------------------------------------

    #[test]
    fn day_shift_positive_offset() {
        // UTC+9 (KST), 4am boundary: 9*60 - 4*60 = 300
        assert_eq!(logical_day_shift_for_offset(9 * 60, 4), "+300 minutes");
    }

    #[test]
    fn day_shift_negative_offset() {
        // UTC-5 (US Eastern standard), 4am boundary: -5*60 - 4*60 = -540
        assert_eq!(logical_day_shift_for_offset(-5 * 60, 4), "-540 minutes");
    }

    // --- sqlite_datetime -------------------------------------------------------

    #[test]
    fn sqlite_datetime_has_no_t_or_zone() {
        let dt = Utc.with_ymd_and_hms(2026, 3, 15, 4, 5, 6).unwrap();
        assert_eq!(sqlite_datetime(dt), "2026-03-15 04:05:06");
    }

    // --- accuracy_percentage ---------------------------------------------------

    #[test]
    fn accuracy_percentage_no_reviews_is_none() {
        assert_eq!(accuracy_percentage(0, 0), None);
    }

    #[test]
    fn accuracy_percentage_all_correct() {
        assert_eq!(accuracy_percentage(5, 5), Some(100));
    }

    #[test]
    fn accuracy_percentage_truncates_not_rounds() {
        // 2/3 = 66.67%, truncated to 66, not rounded to 67
        assert_eq!(accuracy_percentage(2, 3), Some(66));
    }

    #[test]
    fn accuracy_percentage_zero_correct() {
        assert_eq!(accuracy_percentage(0, 5), Some(0));
    }

    // --- parse_flexible_datetime -------------------------------------------

    #[test]
    fn parses_rfc3339_utc() {
        let dt = parse_flexible_datetime("2026-03-15T04:05:06Z").unwrap();
        assert_eq!(dt, Utc.with_ymd_and_hms(2026, 3, 15, 4, 5, 6).unwrap());
    }

    #[test]
    fn parses_rfc3339_with_non_utc_offset_and_converts() {
        // +09:00 (KST) 13:05:06 is 04:05:06 UTC
        let dt = parse_flexible_datetime("2026-03-15T13:05:06+09:00").unwrap();
        assert_eq!(dt, Utc.with_ymd_and_hms(2026, 3, 15, 4, 5, 6).unwrap());
    }

    #[test]
    fn parses_rfc3339_with_fractional_seconds() {
        // Fractional seconds are preserved, not truncated - compare against
        // a value with the same .123 milliseconds rather than the whole
        // second, which would never be equal.
        let dt = parse_flexible_datetime("2026-03-15T04:05:06.123Z").unwrap();
        assert_eq!(
            dt,
            Utc.with_ymd_and_hms(2026, 3, 15, 4, 5, 6).unwrap() + chrono::Duration::milliseconds(123)
        );
    }

    #[test]
    fn parses_sqlite_datetime_no_fraction() {
        let dt = parse_flexible_datetime("2026-03-15 04:05:06").unwrap();
        assert_eq!(dt, Utc.with_ymd_and_hms(2026, 3, 15, 4, 5, 6).unwrap());
    }

    #[test]
    fn parses_sqlite_datetime_with_fraction() {
        let dt = parse_flexible_datetime("2026-03-15 04:05:06.500").unwrap();
        assert_eq!(
            dt,
            Utc.with_ymd_and_hms(2026, 3, 15, 4, 5, 6).unwrap() + chrono::Duration::milliseconds(500)
        );
    }

    #[test]
    fn parses_t_separated_naive_with_fraction() {
        let dt = parse_flexible_datetime("2026-03-15T04:05:06.500").unwrap();
        assert_eq!(
            dt,
            Utc.with_ymd_and_hms(2026, 3, 15, 4, 5, 6).unwrap() + chrono::Duration::milliseconds(500)
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_flexible_datetime("not a date").is_err());
    }

    #[test]
    fn rejects_date_only() {
        assert!(parse_flexible_datetime("2026-03-15").is_err());
    }
}
