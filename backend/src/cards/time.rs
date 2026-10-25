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
fn resolve_local<Tz: TimeZone>(zone: &Tz, naive: NaiveDateTime) -> chrono::DateTime<Utc> {
    match zone.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),
        chrono::LocalResult::Ambiguous(earlier, _later) => earlier.with_timezone(&Utc),
        chrono::LocalResult::None => {
            for hours in 1..=4 {
                let shifted = naive + chrono::Duration::hours(hours);
                if let chrono::LocalResult::Single(dt) = zone.from_local_datetime(&shifted) {
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
    logical_day_start(&Local::now(), day_boundary_hour, 0)
}

/// Logical date for a local wall-clock timestamp. Shared by current-day
/// streak checks and historical review grouping so they use the same boundary.
pub(super) fn logical_today_date(now_local: NaiveDateTime, day_boundary_hour: i64) -> NaiveDate {
    logical_day_start_naive(now_local, day_boundary_hour).date()
}

/// Format a UTC datetime for comparison against SQLite datetime() values.
pub(super) fn sqlite_datetime(dt: chrono::DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Resolve a calendar day's boundary in the supplied timezone. Calendar
/// offsets preserve the boundary hour across 23- and 25-hour days.
pub(super) fn logical_day_start<Tz: TimeZone>(
    now: &chrono::DateTime<Tz>, day_boundary_hour: i64, days: i64,
) -> chrono::DateTime<Utc> {
    let naive = logical_day_start_naive(now.naive_local(), day_boundary_hour)
        + chrono::Duration::days(days);
    resolve_local(&now.timezone(), naive)
}

pub(super) fn logical_next_start(day_boundary_hour: i64) -> chrono::DateTime<Utc> {
    logical_day_start(&Local::now(), day_boundary_hour, 1)
}

/// Assign a stored UTC instant using the timezone rules at that instant,
/// rather than applying today's offset to historical reviews.
pub(super) fn review_day<Tz: TimeZone>(
    reviewed_at: &str, day_boundary_hour: i64, zone: &Tz,
) -> Result<NaiveDate, chrono::ParseError> {
    let local = parse_flexible_datetime(reviewed_at)?.with_timezone(zone);
    Ok(logical_today_date(local.naive_local(), day_boundary_hour))
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

    fn utc(s: &str) -> chrono::DateTime<Utc> {
        parse_flexible_datetime(s).unwrap()
    }

    #[test]
    fn next_boundary_tracks_short_and_long_calendar_days() {
        let zone = chrono_tz::Europe::London;
        for (now, expected, hours) in [
            ("2026-03-28T12:00:00Z", "2026-03-29T03:00:00Z", 23),
            ("2026-10-24T12:00:00Z", "2026-10-25T04:00:00Z", 25),
        ] {
            let now = utc(now).with_timezone(&zone);
            let start = logical_day_start(&now, 4, 0);
            let next = logical_day_start(&now, 4, 1);
            assert_eq!(next, utc(expected));
            assert_eq!((next - start).num_hours(), hours);
        }
    }

    #[test]
    fn boundary_inside_repeated_hour_uses_first_occurrence() {
        let zone = chrono_tz::Europe::London;
        assert_eq!(resolve_local(&zone, ndt(2026, 10, 25, 1, 0, 0)), utc("2026-10-25T00:00:00Z"));
        for instant in ["2026-10-25T00:30:00Z", "2026-10-25T01:30:00Z"] {
            let now = utc(instant).with_timezone(&zone);
            assert_eq!(logical_day_start(&now, 1, 0), utc("2026-10-25T00:00:00Z"));
            assert_eq!(review_day(instant, 1, &zone).unwrap(), ndt(2026, 10, 25, 0, 0, 0).date());
        }
    }

    #[test]
    fn missing_boundary_moves_forward_and_agrees_with_review_day() {
        let zone = chrono_tz::Europe::London;
        assert_eq!(resolve_local(&zone, ndt(2026, 3, 29, 1, 0, 0)), utc("2026-03-29T01:00:00Z"));
        for (instant, expected) in [
            ("2026-03-29T00:59:59Z", "2026-03-28T01:00:00Z"),
            ("2026-03-29T01:00:00Z", "2026-03-29T01:00:00Z"),
        ] {
            let now = utc(instant).with_timezone(&zone);
            let start = logical_day_start(&now, 1, 0);
            assert_eq!(start, utc(expected));
            assert_eq!(review_day(instant, 1, &zone).unwrap(), start.with_timezone(&zone).date_naive());
        }
    }

    #[test]
    fn historical_review_days_follow_each_instants_offset() {
        let zone = chrono_tz::Europe::London;
        for (instant, expected) in [
            ("2026-03-28T03:59:59Z", "2026-03-27"),
            ("2026-03-28T04:00:00Z", "2026-03-28"),
            ("2026-03-29T02:59:59Z", "2026-03-28"),
            ("2026-03-29T03:00:00Z", "2026-03-29"),
            ("2026-10-24T02:59:59Z", "2026-10-23"),
            ("2026-10-24T03:00:00Z", "2026-10-24"),
            ("2026-10-25T03:59:59Z", "2026-10-24"),
            ("2026-10-25T04:00:00Z", "2026-10-25"),
        ] {
            assert_eq!(review_day(instant, 4, &zone).unwrap().to_string(), expected);
        }
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
