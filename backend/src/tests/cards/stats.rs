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

async fn insert_review(
    pool: &SqlitePool,
    user_id: i64,
    card_id: i64,
    rating: &str,
    state: &str,
    reviewed_at: &str,
) {
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
async fn autumn_history_and_streaks_use_historical_offsets() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    for (timestamp, rating, state) in [
        ("2026-10-24 02:59:59", "good", "review"),
        ("2026-10-24 03:00:00", "good", "review"),
        ("2026-10-24T03:30:00Z", "again", "review"),
        ("2026-10-25 03:59:59", "good", "review"),
        ("2026-10-25 04:00:00", "good", "review"),
        ("2026-10-26 04:00:00", "good", "learning"),
    ] {
        insert_review(&pool, user_id, 1, rating, state, timestamp).await;
    }
    let zone = chrono_tz::Europe::London;
    let now = parse_flexible_datetime("2026-10-27T12:00:00Z")
        .unwrap()
        .with_timezone(&zone);
    let days = query_timeseries_at(&pool, user_id, 4, &now).await.unwrap();
    let counts: Vec<_> = days
        .iter()
        .map(|day| (day.date.as_str(), day.total, day.correct))
        .collect();
    assert_eq!(
        counts,
        vec![
            ("2026-10-23", 1, 1),
            ("2026-10-24", 3, 2),
            ("2026-10-25", 1, 1)
        ]
    );
    assert!((days[1].percentage - 66.66666666666667).abs() < 1e-12);
    // A window starting before the fall-back must include 4am BST,
    // rather than starting an hour late after subtracting 96 hours.
    let later = parse_flexible_datetime("2026-10-28T12:00:00Z")
        .unwrap()
        .with_timezone(&zone);
    let later_days = query_timeseries_at(&pool, user_id, 4, &later)
        .await
        .unwrap();
    assert_eq!(later_days[0].date, "2026-10-24");
    assert_eq!(later_days[0].total, 3);
    let dates = query_review_dates(&pool, user_id, 4, &zone).await.unwrap();
    assert_eq!(
        dates,
        vec![
            d("2026-10-23"),
            d("2026-10-24"),
            d("2026-10-25"),
            d("2026-10-26")
        ]
    );
    assert_eq!(compute_streaks(&dates, d("2026-10-26")), (4, 4));
}

#[tokio::test]
async fn spring_history_window_starts_at_local_boundary() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    for timestamp in [
        "2026-03-28 03:30:00",
        "2026-03-28 04:00:00",
        "2026-03-29 03:30:00",
    ] {
        insert_review(&pool, user_id, 1, "good", "review", timestamp).await;
    }
    let zone = chrono_tz::Europe::London;
    let now = parse_flexible_datetime("2026-04-01T12:00:00Z")
        .unwrap()
        .with_timezone(&zone);
    let days = query_timeseries_at(&pool, user_id, 4, &now).await.unwrap();
    let counts: Vec<_> = days
        .iter()
        .map(|day| (day.date.as_str(), day.total))
        .collect();
    assert_eq!(counts, vec![("2026-03-28", 1), ("2026-03-29", 1)]);
}

#[tokio::test]
async fn stats_use_each_users_saved_timezone() {
    let pool = test_pool().await;
    let now = Utc::now();
    // Boundaries opposite each other ensure today's counts differ at any UTC hour.
    let zones = [chrono_tz::Asia::Seoul, chrono_tz::America::Sao_Paulo];
    let starts = zones.map(|zone| logical_day_start(&now.with_timezone(&zone), 4, 0));
    let review_at = sqlite_datetime(starts[0].min(starts[1]) + chrono::Duration::hours(6));
    for (zone, start) in zones.into_iter().zip(starts) {
        let user_id = test_user(&pool).await;
        sqlx::query("INSERT INTO users_settings (user_id, study_timezone) VALUES (?, ?)")
            .bind(user_id)
            .bind(zone.name())
            .execute(&pool)
            .await
            .unwrap();
        insert_review(&pool, user_id, 1, "good", "review", &review_at).await;
        let stats = get_stats(State(pool.clone()), crate::auth::AuthUser(user_id))
            .await
            .unwrap()
            .0;
        assert_eq!(
            stats.reviews_today,
            i64::from(review_at >= sqlite_datetime(start))
        );
        let history = get_history(State(pool.clone()), crate::auth::AuthUser(user_id))
            .await
            .unwrap()
            .0;
        assert_eq!(history.timeseries.len(), 1);
        assert_eq!(
            history.timeseries[0].date,
            review_day(&review_at, 4, &zone).unwrap().to_string()
        );
        assert_eq!(history.summary.current_streak, stats.reviews_today);
        assert_eq!(history.summary.longest_streak, 1);
    }
}

#[tokio::test]
async fn reviews_before_the_day_boundary_dont_count_as_today() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;

    // Same boundary the handler will compute (no users_settings row -> default hour 4).
    let boundary = logical_today_start(4, &chrono_tz::UTC);
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
    let boundary = logical_today_start(4, &chrono_tz::UTC);
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
    assert_eq!(stats.percentage, Some(0.0));
}

#[tokio::test]
async fn stats_and_history_serialize_unrounded_accuracy() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let today =
        sqlite_datetime(logical_today_start(4, &chrono_tz::UTC) + chrono::Duration::hours(1));
    for index in 0..9 {
        let rating = if index < 8 { "good" } else { "again" };
        insert_review(&pool, user_id, 1, rating, "review", &today).await;
    }

    let stats = get_stats(State(pool.clone()), crate::auth::AuthUser(user_id))
        .await
        .unwrap()
        .0;
    let history = get_history(State(pool), crate::auth::AuthUser(user_id))
        .await
        .unwrap()
        .0;
    let stats_json = serde_json::to_value(stats).unwrap();
    let history_json = serde_json::to_value(history).unwrap();
    for percentage in [
        &stats_json["percentage"],
        &history_json["timeseries"][0]["percentage"],
    ] {
        assert!((percentage.as_f64().unwrap() - 88.88888888888889).abs() < 1e-12);
    }
}

#[tokio::test]
async fn history_counts_only_the_users_currently_suppressed_cards() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let other_user_id = test_user(&pool).await;

    let history = get_history(State(pool.clone()), crate::auth::AuthUser(user_id))
        .await
        .unwrap()
        .0;
    assert_eq!(history.summary.cards_suppressed, 0);

    // Suppression includes both reviewed and unseen cards, and excludes
    // cards that were unsuppressed or belong to another user.
    insert_review(&pool, user_id, 1, "good", "review", "2026-03-15 12:00:00").await;
    for (owner, card_id, suppressed) in [
        (user_id, 1, 1),
        (user_id, 2, 1),
        (user_id, 3, 0),
        (other_user_id, 3, 1),
    ] {
        sqlx::query("INSERT INTO users_card_flags (user_id, card_id, suppressed) VALUES (?, ?, ?)")
            .bind(owner)
            .bind(card_id)
            .bind(suppressed)
            .execute(&pool)
            .await
            .unwrap();
    }

    let history = get_history(State(pool.clone()), crate::auth::AuthUser(user_id))
        .await
        .unwrap()
        .0;
    assert_eq!(history.summary.cards_suppressed, 2);
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
