use super::*;

// --- split_sentence -----------------------------------------------------

#[test]
fn splits_target_at_start() {
    let (before, after) = split_sentence("먹다 좋아해요", "먹다");
    assert_eq!(before, "");
    assert_eq!(after, " 좋아해요");
}

#[test]
fn splits_target_in_middle() {
    let (before, after) = split_sentence("저는 먹다 좋아해요", "먹다");
    assert_eq!(before, "저는 ");
    assert_eq!(after, " 좋아해요");
}

#[test]
fn splits_target_at_end() {
    let (before, after) = split_sentence("저는 먹다", "먹다");
    assert_eq!(before, "저는 ");
    assert_eq!(after, "");
}

#[test]
fn falls_back_to_whole_sentence_when_target_absent() {
    let (before, after) = split_sentence("저는 밥을 먹어요", "먹다");
    assert_eq!(before, "저는 밥을 먹어요");
    assert_eq!(after, "");
}

#[test]
fn splits_at_first_occurrence_when_target_repeats() {
    let (before, after) = split_sentence("가다 가다 가다", "가다");
    assert_eq!(before, "");
    assert_eq!(after, " 가다 가다");
}

#[test]
fn empty_target_splits_at_start() {
    let (before, after) = split_sentence("안녕하세요", "");
    assert_eq!(before, "");
    assert_eq!(after, "안녕하세요");
}

use crate::auth::AuthUser;
use crate::cards::time::parse_flexible_datetime;
use crate::test_support::{test_pool, test_review_user as test_user};

async fn queue(pool: &SqlitePool, user_id: i64, limit: i64) -> ReviewQueueResponse {
    get_review_queue(
        State(pool.clone()),
        AuthUser(user_id),
        AppQuery(ReviewQueueQuery { limit: Some(limit) }),
    )
    .await
    .unwrap()
    .0
}

fn ids(queue: &ReviewQueueResponse) -> Vec<i64> {
    queue.cards.iter().map(|c| c.card.card_id).collect()
}

#[tokio::test]
async fn saved_timezone_controls_new_card_allowance_and_reset() {
    let pool = test_pool().await;
    let now = chrono::Utc::now();
    let zones = [chrono_tz::Asia::Seoul, chrono_tz::America::Sao_Paulo];
    let starts = zones.map(|zone| logical_day_start(&now.with_timezone(&zone), 4, 0));
    let reviewed_at = starts[0].min(starts[1]) + chrono::Duration::hours(6);
    for (zone, start) in zones.into_iter().zip(starts) {
        let user = test_user(&pool).await;
        sqlx::query("INSERT INTO users_settings (user_id, daily_new_card_limit, study_timezone) VALUES (?, 1, ?) ON CONFLICT(user_id) DO UPDATE SET daily_new_card_limit = excluded.daily_new_card_limit, study_timezone = excluded.study_timezone")
            .bind(user).bind(zone.name()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO review_history (user_id, card_id, rating, state, reviewed_at) VALUES (?, 1, 'good', 'learning', ?)")
            .bind(user).bind(sqlite_datetime(reviewed_at)).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO cards_states (user_id, card_id, stability, difficulty, state, last_review, due_at) VALUES (?, 1, 10, 1, 'review', ?, ?)")
            .bind(user).bind(sqlite_datetime(reviewed_at)).bind((now + chrono::Duration::days(10)).timestamp())
            .execute(&pool).await.unwrap();
        let response = queue(&pool, user, 5).await;
        let limited = reviewed_at >= start;
        assert_eq!(response.cards.is_empty(), limited);
        if limited {
            assert_eq!(
                parse_flexible_datetime(response.next_available_at.as_deref().unwrap()).unwrap(),
                logical_day_start(&now.with_timezone(&zone), 4, 1)
            );
        }
    }
}

#[tokio::test]
async fn omitted_limit_returns_all_due_cards_and_allowance_bounded_new_cards() {
    let pool = test_pool().await;
    let user = test_user(&pool).await;
    sqlx::query("INSERT INTO users_settings (user_id, daily_new_card_limit) VALUES (?, 3) ON CONFLICT(user_id) DO UPDATE SET daily_new_card_limit = excluded.daily_new_card_limit")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO review_history (user_id, card_id, rating, reviewed_at, stability, difficulty, state) SELECT ?, id, 'good', datetime('now', '-10 days'), 10, 1, 'review' FROM cards WHERE id <= 25")
        .bind(user).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at) SELECT ?, id, 10, 1, datetime('now', '-10 days'), 'review', 0 FROM cards WHERE id <= 25")
        .bind(user).execute(&pool).await.unwrap();
    let all = get_review_queue(
        State(pool.clone()),
        AuthUser(user),
        AppQuery(ReviewQueueQuery::default()),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(all.cards.len(), 28);
    assert!(ids(&all)[..25].iter().all(|id| *id <= 25));
    assert_eq!(ids(&queue(&pool, user, 5).await), ids(&all)[..5]);
    assert_eq!(ids(&queue(&pool, user, 100).await), ids(&all));
}

#[tokio::test]
async fn batch_clients_share_the_same_allowance_bounded_set() {
    let pool = test_pool().await;
    let user = test_user(&pool).await;
    sqlx::query("INSERT INTO users_settings (user_id, daily_new_card_limit) VALUES (?, 1) ON CONFLICT(user_id) DO UPDATE SET daily_new_card_limit = excluded.daily_new_card_limit")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE cards SET frequency_rank = NULL")
        .execute(&pool)
        .await
        .unwrap();
    let (a, b, c) = tokio::join!(
        queue(&pool, user, 1),
        queue(&pool, user, 5),
        queue(&pool, user, 20)
    );
    assert_eq!(ids(&a), vec![1]);
    assert_eq!(ids(&a), ids(&b));
    assert_eq!(ids(&b), ids(&c));
    assert!(a.next_available_at.is_none());
}

#[tokio::test]
async fn out_of_order_first_reviews_do_not_expand_the_new_card_set() {
    let pool = test_pool().await;
    let user = test_user(&pool).await;
    sqlx::query("INSERT INTO users_settings (user_id, daily_new_card_limit) VALUES (?, 3) ON CONFLICT(user_id) DO UPDATE SET daily_new_card_limit = excluded.daily_new_card_limit")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE cards SET frequency_rank = NULL")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(ids(&queue(&pool, user, 20).await), vec![1, 2, 3]);
    for (card, expected) in [(2, vec![1, 3]), (1, vec![3]), (3, vec![])] {
        sqlx::query("INSERT INTO review_history (user_id, card_id, rating, reviewed_at, stability, difficulty, state) VALUES (?, ?, 'good', datetime('now'), 10, 1, 'learning')")
            .bind(user).bind(card).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at) VALUES (?, ?, 10, 1, datetime('now'), 'learning', unixepoch('now', '+10 days'))")
            .bind(user).bind(card).execute(&pool).await.unwrap();
        assert_eq!(ids(&queue(&pool, user, 20).await), expected);
    }
    assert!(queue(&pool, user, 5).await.next_available_at.is_some());
    // A due repetition is still eligible after exhausting new cards.
    sqlx::query("UPDATE cards_states SET due_at = unixepoch('now', '-1 minute') WHERE user_id = ? AND card_id = 2")
        .bind(user).execute(&pool).await.unwrap();
    assert_eq!(ids(&queue(&pool, user, 5).await), vec![2]);
}

#[tokio::test]
async fn batch_deadline_tracks_reviews_that_become_due_before_the_day_boundary() {
    let pool = test_pool().await;
    let user = test_user(&pool).await;
    sqlx::query("INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at) VALUES (?, 1, 10, 1, datetime('now'), 'learning', unixepoch('now', '+2 minutes'))")
        .bind(user).execute(&pool).await.unwrap();
    let response = queue(&pool, user, 5).await;
    let due: String = sqlx::query_scalar("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', due_at, 'unixepoch') FROM cards_states WHERE user_id = ? AND card_id = 1")
        .bind(user).fetch_one(&pool).await.unwrap();
    assert!(response.next_available_at.as_ref().unwrap() <= &due);
    assert!(!ids(&response).contains(&1));
    let again = queue(&pool, user, 5).await;
    assert_eq!(
        serde_json::to_value(&response.cards).unwrap(),
        serde_json::to_value(&again.cards).unwrap()
    );
    let prompt = serde_json::to_value(&response.cards[0]).unwrap();
    assert!(prompt.get("word").is_none());
    assert!(prompt.get("target").is_none());
    assert!(prompt["card"].get("sentence_id").is_some());
    assert!(prompt["card"].get("word").is_none());
    assert!(prompt["card"].get("target").is_none());
    assert!(prompt["review"].get("attempt_token").is_some());
    assert!(prompt["card"].get("attempt_token").is_none());
}

#[tokio::test]
async fn batch_size_is_validated_and_zero_daily_limit_is_empty() {
    let pool = test_pool().await;
    let user = test_user(&pool).await;
    for limit in [-1, 0] {
        assert!(matches!(
            get_review_queue(
                State(pool.clone()),
                AuthUser(user),
                AppQuery(ReviewQueueQuery { limit: Some(limit) }),
            )
            .await,
            Err(AppError::BadRequest(_))
        ));
    }
    sqlx::query("INSERT INTO users_settings (user_id, daily_new_card_limit) VALUES (?, 0) ON CONFLICT(user_id) DO UPDATE SET daily_new_card_limit = excluded.daily_new_card_limit")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let result = queue(&pool, user, 5).await;
    assert!(result.cards.is_empty());
    assert!(result.next_available_at.is_none());
}

/// Marks every seeded card except `keep` as already-reviewed and due far
/// in the future - used to isolate a single card as the only "new" (or
/// only "due-later") candidate without hand-seeding all 50.
async fn bury_every_other_card(pool: &SqlitePool, user_id: i64, keep: i64) {
    sqlx::query(
        r#"
        INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at)
        SELECT ?, id, 1000, 0, datetime('now'), 'review', unixepoch('now', '+1000 days') FROM cards WHERE id != ?
        "#,
    )
    .bind(user_id)
    .bind(keep)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn due_card_is_preferred_over_new_cards() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;

    // Card 2 has a persisted deadline in the past;
    // every other card (including card 1) is untouched, i.e. "new".
    sqlx::query(
        "INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at) \
         VALUES (?, 2, 1, 0, datetime('now', '-10 days'), 'review', unixepoch('now', '-9 days'))",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    let envelope = queue(&pool, user_id, 1).await;
    let card = envelope.cards.first().expect("a card should be due");
    assert_eq!(card.card.card_id, 2);
}

#[tokio::test]
async fn suppressed_card_is_never_returned() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;

    // Card 2 is due, but suppressed - it must not come back even though
    // it would otherwise win on priority.
    sqlx::query(
        "INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at) \
         VALUES (?, 2, 1, 0, datetime('now', '-10 days'), 'review', unixepoch('now', '-9 days'))",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO users_card_flags (user_id, card_id, suppressed) VALUES (?, 2, 1)")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    let envelope = queue(&pool, user_id, 1).await;
    let card = envelope
        .cards
        .first()
        .expect("some other card should still be available");
    assert_ne!(card.card.card_id, 2);
}

#[tokio::test]
async fn daily_new_card_limit_zero_suppresses_all_new_cards() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    sqlx::query("INSERT INTO users_settings (user_id, daily_new_card_limit) VALUES (?, 0) ON CONFLICT(user_id) DO UPDATE SET daily_new_card_limit = excluded.daily_new_card_limit")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    // No card has any review_history, so with new cards suppressed and
    // none due, nothing is available.
    let envelope = queue(&pool, user_id, 1).await;
    assert!(envelope.cards.is_empty());
    assert!(
        envelope.next_available_at.is_none(),
        "a zero limit does not reset tomorrow"
    );
}

#[tokio::test]
async fn next_available_at_reports_the_earliest_scheduled_due_date() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;

    // Every card has a cards_states row (so none reads as "new"), and
    // card 1's is due soonest.
    bury_every_other_card(&pool, user_id, 1).await;
    sqlx::query(
        "INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at) \
         VALUES (?, 1, 3, 0, datetime('now'), 'review', unixepoch('now', '+3 days'))",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    let last_review: String = sqlx::query_scalar(
        "SELECT last_review FROM cards_states WHERE user_id = ? AND card_id = 1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let expected = (parse_flexible_datetime(&last_review).unwrap() + chrono::Duration::days(3))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let envelope = queue(&pool, user_id, 1).await;
    assert!(envelope.cards.is_empty(), "nothing should be due yet");
    assert_eq!(envelope.next_available_at, Some(expected));
}

#[tokio::test]
async fn prompts_return_current_revision_and_due_again_accepts_it() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    bury_every_other_card(&pool, user_id, 1).await;
    let fetch = || {
        get_review_queue(
            State(pool.clone()),
            AuthUser(user_id),
            AppQuery(ReviewQueueQuery::default()),
        )
    };
    let card = fetch().await.unwrap().0.cards.into_iter().next().unwrap();
    assert_eq!(
        card.review.attempt_token,
        crate::cards::issue_attempt_token(user_id, 1, card.card.sentence_id, 0, 0, "eng").unwrap()
    );
    assert_eq!(
        fetch()
            .await
            .unwrap()
            .0
            .cards
            .into_iter()
            .next()
            .unwrap()
            .review
            .attempt_token,
        crate::cards::issue_attempt_token(user_id, 1, 1, 0, 0, "eng").unwrap()
    );
    let answer: String = sqlx::query_scalar("SELECT tg.form FROM sentences s JOIN targets tg ON tg.sentence_id = s.id WHERE s.card_id = 1")
        .fetch_one(&pool).await.unwrap();
    let _ = super::super::check::check_answer(
        State(pool.clone()),
        AuthUser(user_id),
        crate::error::AppJson(super::super::check::CheckRequest {
            answer: answer.clone(),
            attempt_token: crate::cards::issue_attempt_token(user_id, 1, 1, 0, 0, "eng").unwrap(),
        }),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE cards_states SET last_review = '2000-01-01T00:00:00Z', due_at = 0 WHERE user_id = ? AND card_id = 1")
        .bind(user_id).execute(&pool).await.unwrap();
    let card = fetch().await.unwrap().0.cards.into_iter().next().unwrap();
    assert_eq!(
        card.review.attempt_token,
        crate::cards::issue_attempt_token(user_id, 1, card.card.sentence_id, 1, 0, "eng").unwrap()
    );
    let _ = super::super::check::check_answer(
        State(pool.clone()),
        AuthUser(user_id),
        crate::error::AppJson(super::super::check::CheckRequest {
            answer,
            attempt_token: card.review.attempt_token.clone(),
        }),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn repeated_reviews_keep_the_cards_single_sentence() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    bury_every_other_card(&pool, user_id, 1).await;
    let first_id: i64 = sqlx::query_scalar("SELECT MIN(id) FROM sentences WHERE card_id = 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let first_answer: String = sqlx::query_scalar("SELECT form FROM targets WHERE sentence_id = ?")
        .bind(first_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let fetch = || {
        get_review_queue(
            State(pool.clone()),
            AuthUser(user_id),
            AppQuery(ReviewQueueQuery::default()),
        )
    };

    for (expected_id, answer, revision) in [
        (first_id, first_answer.as_str(), 0),
        (first_id, first_answer.as_str(), 1),
        (first_id, first_answer.as_str(), 2),
    ] {
        let card = fetch().await.unwrap().0.cards.into_iter().next().unwrap();
        assert_eq!(card.card.card_id, 1);
        assert_eq!(card.card.sentence_id, expected_id);
        assert_eq!(
            card.review.attempt_token,
            crate::cards::issue_attempt_token(user_id, 1, expected_id, revision, 0, "eng").unwrap()
        );
        let result = super::super::check::check_answer(
            State(pool.clone()),
            AuthUser(user_id),
            crate::error::AppJson(super::super::check::CheckRequest {
                answer: answer.into(),
                attempt_token: crate::cards::issue_attempt_token(
                    user_id,
                    1,
                    expected_id,
                    revision,
                    0,
                    "eng",
                )
                .unwrap(),
            }),
        )
        .await
        .unwrap();
        assert!(result.1.0.correct);
        sqlx::query("UPDATE cards_states SET due_at = 0 WHERE user_id = ? AND card_id = 1")
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap();
    }
    let recorded: Vec<i64> = sqlx::query_scalar(
        "SELECT sentence_id FROM review_history WHERE user_id = ? AND card_id = 1 ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(recorded, vec![first_id, first_id, first_id]);
}

#[tokio::test]
async fn queue_and_validation_skip_sentences_without_the_requested_translation() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    bury_every_other_card(&pool, user_id, 1).await;
    sqlx::query("INSERT INTO languages (id, slug) VALUES (100, 'fra')")
        .execute(&pool)
        .await
        .unwrap();
    let queue = get_review_queue(
        State(pool.clone()),
        AuthUser(user_id),
        AppQuery(ReviewQueueQuery::default()),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(queue.cards.len(), 1);
    let card = &queue.cards[0];
    assert_ne!(card.card.sentence_id, -1);
    let mut tx = pool.begin().await.unwrap();
    crate::cards::validate_attempt(&mut tx, &card.review.attempt_token, user_id, Some(1))
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    sqlx::query("UPDATE users_settings SET translation_language = 'fra' WHERE user_id = ?")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        get_review_queue(
            State(pool.clone()),
            AuthUser(user_id),
            AppQuery(ReviewQueueQuery::default())
        )
        .await
        .unwrap()
        .0
        .cards
        .is_empty()
    );
}

#[tokio::test]
async fn review_context_keeps_optional_hints_separate_from_card_content() {
    let pool = test_pool().await;
    let user = test_user(&pool).await;
    sqlx::query("UPDATE cards SET hanja = '學' WHERE id IN (1, 2)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO cards_states (user_id, card_id, state) VALUES (?, 2, 'review')")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let enabled = queue(&pool, user, 50).await;
    let entry = enabled
        .cards
        .iter()
        .find(|entry| entry.card.card_id == 1)
        .unwrap();
    assert_eq!(
        entry.review.hanja_hint_words.as_ref().unwrap(),
        &vec!["學".to_string()]
    );
    let card_content = serde_json::to_value(&entry.card).unwrap();
    assert!(card_content.get("hanja_hint_words").is_none());
    assert!(card_content.get("attempt_token").is_none());
    crate::user::ensure_settings_row(&pool, user).await.unwrap();
    sqlx::query("UPDATE users_settings SET show_hanja_hints = 0 WHERE user_id = ?")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let disabled = queue(&pool, user, 50).await;
    let entry = disabled
        .cards
        .iter()
        .find(|entry| entry.card.card_id == 1)
        .unwrap();
    assert_eq!(serde_json::to_value(&entry.card).unwrap(), card_content);
    assert!(entry.review.hanja_hint_words.is_none());
    let body = serde_json::to_value(entry).unwrap();
    assert!(body["review"]["hanja_hint_words"].is_null());
}

#[tokio::test]
async fn availability_is_independent_of_queue_size_and_ignores_disabled_new_card_reset() {
    let pool = test_pool().await;
    let user = test_user(&pool).await;
    sqlx::query("UPDATE users_settings SET daily_new_card_limit = 0 WHERE user_id = ?")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    for (card, days) in [(1, -1), (2, 3)] {
        sqlx::query("INSERT INTO review_history (user_id,card_id,rating,state) VALUES (?,?,'good','review')")
            .bind(user).bind(card).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO cards_states (user_id,card_id,last_review,state,due_at) VALUES (?,?,datetime('now'),'review',unixepoch('now') + ? * 86400)")
            .bind(user).bind(card).bind(days).execute(&pool).await.unwrap();
    }
    let expected: String = sqlx::query_scalar("SELECT strftime('%Y-%m-%dT%H:%M:%SZ',due_at,'unixepoch') FROM cards_states WHERE user_id = ? AND card_id = 2")
        .bind(user).fetch_one(&pool).await.unwrap();
    let available = queue(&pool, user, 1).await;
    assert_eq!(ids(&available), vec![1]);
    assert_eq!(
        available.next_available_at.as_deref(),
        Some(expected.as_str())
    );
    let json = serde_json::to_value(available).unwrap();
    assert!(json.get("refresh_at").is_none());
    assert!(json.get("next_due_at").is_none());
    sqlx::query("UPDATE cards_states SET due_at = 0 WHERE user_id = ? AND card_id = 2")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let available = queue(&pool, user, 1).await;
    assert_eq!(available.cards.len(), 1);
    assert!(
        available.next_available_at.is_none(),
        "already eligible cards outside the response limit are not future availability"
    );
}

#[tokio::test]
async fn allowance_reset_only_reports_new_cards_with_eligible_content() {
    let pool = test_pool().await;
    let user = test_user(&pool).await;
    // All cards are immediately available; the reset unlocks nothing.
    sqlx::query("UPDATE users_settings SET daily_new_card_limit = 100 WHERE user_id = ?")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    assert!(queue(&pool, user, 1).await.next_available_at.is_none());
    sqlx::query("UPDATE users_settings SET daily_new_card_limit = 1 WHERE user_id = ?")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        queue(&pool, user, 1).await.next_available_at.is_none(),
        "an unused allowance does not expand at the boundary"
    );
    sqlx::query(
        "INSERT INTO review_history(user_id,card_id,rating,state) VALUES (?,1,'good','learning')",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    assert!(queue(&pool, user, 1).await.next_available_at.is_some());
    sqlx::query("INSERT INTO users_card_flags(user_id,card_id,suppressed) SELECT ?,id,1 FROM cards WHERE id > 1")
        .bind(user).execute(&pool).await.unwrap();
    assert!(queue(&pool, user, 1).await.next_available_at.is_none());
    sqlx::query("DELETE FROM users_card_flags WHERE user_id = ?")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO languages(slug) VALUES ('fra')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE users_settings SET translation_language = 'fra' WHERE user_id = ?")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let empty = queue(&pool, user, 1).await;
    assert!(empty.cards.is_empty());
    assert!(empty.next_available_at.is_none());
}
