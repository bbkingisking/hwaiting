use super::*;
use chrono::NaiveDate;

fn ndt(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, mo, d)
        .unwrap()
        .and_hms_opt(h, mi, s)
        .unwrap()
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
    assert_eq!(
        resolve_local(&zone, ndt(2026, 10, 25, 1, 0, 0)),
        utc("2026-10-25T00:00:00Z")
    );
    for instant in ["2026-10-25T00:30:00Z", "2026-10-25T01:30:00Z"] {
        let now = utc(instant).with_timezone(&zone);
        assert_eq!(logical_day_start(&now, 1, 0), utc("2026-10-25T00:00:00Z"));
        assert_eq!(
            review_day(instant, 1, &zone).unwrap(),
            ndt(2026, 10, 25, 0, 0, 0).date()
        );
    }
}

#[test]
fn missing_boundary_moves_forward_and_agrees_with_review_day() {
    let zone = chrono_tz::Europe::London;
    assert_eq!(
        resolve_local(&zone, ndt(2026, 3, 29, 1, 0, 0)),
        utc("2026-03-29T01:00:00Z")
    );
    for (instant, expected) in [
        ("2026-03-29T00:59:59Z", "2026-03-28T01:00:00Z"),
        ("2026-03-29T01:00:00Z", "2026-03-29T01:00:00Z"),
    ] {
        let now = utc(instant).with_timezone(&zone);
        let start = logical_day_start(&now, 1, 0);
        assert_eq!(start, utc(expected));
        assert_eq!(
            review_day(instant, 1, &zone).unwrap(),
            start.with_timezone(&zone).date_naive()
        );
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
    assert_eq!(accuracy_percentage(5, 5), Some(100.0));
}

#[test]
fn accuracy_percentage_preserves_fractional_precision() {
    for (correct, total, expected) in [(2, 3, 66.66666666666667), (8, 9, 88.88888888888889)] {
        assert!((accuracy_percentage(correct, total).unwrap() - expected).abs() < 1e-12);
    }
}

#[test]
fn accuracy_percentage_zero_correct() {
    assert_eq!(accuracy_percentage(0, 5), Some(0.0));
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
