use super::*;

#[test]
fn exact_match_is_correct() {
    assert!(is_correct("먹어요", "먹어요", &[]));
}

#[test]
fn mismatch_is_incorrect() {
    assert!(!is_correct("먹어요", "먹었어요", &[]));
}

#[test]
fn leading_and_trailing_whitespace_is_trimmed() {
    assert!(is_correct("  먹어요  ", "먹어요", &[]));
}

#[test]
fn matches_an_alternative_target() {
    let alts = vec!["먹었어요".to_string(), "드셨어요".to_string()];
    assert!(is_correct("드셨어요", "먹어요", &alts));
}

// No separate "near miss" / "empty answer" cases: both just exercise the
// same `!=` comparison mismatch_is_incorrect already covers, with
// different data. And no NFD-vs-NFC normalization test either - there's
// no normalization step to test, so a test here could only ever pin
// down the *absence* of a feature, not catch a regression in one.

// --- check_answer (handler-level, in-process SQLite) ---------------------

use crate::auth::AuthUser;
use crate::test_support::{test_pool, test_user};

/// The literal target text for card_id (any seeded sample card, 1-50) -
/// fetched directly rather than hardcoded, so these tests don't need to
/// know the seed data's actual Korean content.
async fn target_for(pool: &SqlitePool, card_id: i64) -> String {
    sqlx::query_scalar(
        "SELECT tg.form FROM sentences s JOIN targets tg ON tg.sentence_id = s.id WHERE s.card_id = ?",
    )
    .bind(card_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn card_state(
    pool: &SqlitePool,
    user_id: i64,
    card_id: i64,
) -> (Option<f64>, Option<f64>, Option<String>, Option<String>) {
    sqlx::query_as(
        "SELECT stability, difficulty, last_review, state FROM cards_states WHERE user_id = ? AND card_id = ?",
    )
    .bind(user_id)
    .bind(card_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn new_card_graded_correct_becomes_learning() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let card_id = 1;
    let target = target_for(&pool, card_id).await;

    let response = check_answer(
        State(pool.clone()),
        AppPath(card_id),
        AuthUser(user_id),
        AppJson(CheckRequest {
            answer: target,
            attempt_token: crate::cards::issue_attempt_token(user_id, card_id, 1, 0, 0).unwrap(),
        }),
    )
    .await
    .unwrap();

    assert!(response.0.correct);

    let (stability, difficulty, last_review, state) = card_state(&pool, user_id, card_id).await;
    assert_eq!(state.as_deref(), Some("learning"));
    assert!(stability.is_some());
    assert!(difficulty.is_some());
    assert!(last_review.is_some());

    let review_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM review_history WHERE user_id = ? AND card_id = ?")
            .bind(user_id)
            .bind(card_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(review_count, 1);

    let rating: String =
        sqlx::query_scalar("SELECT rating FROM review_history WHERE user_id = ? AND card_id = ?")
            .bind(user_id)
            .bind(card_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(rating, "good");
}

#[tokio::test]
async fn new_card_graded_wrong_is_incorrect_but_still_learning() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let card_id = 1;

    let response = check_answer(
        State(pool.clone()),
        AppPath(card_id),
        AuthUser(user_id),
        AppJson(CheckRequest {
            answer: "definitely wrong".to_string(),
            attempt_token: crate::cards::issue_attempt_token(user_id, card_id, 1, 0, 0).unwrap(),
        }),
    )
    .await
    .unwrap();

    assert!(!response.0.correct);

    // A first-ever review is "learning" regardless of correctness - the
    // rating (again vs. good) only changes the FSRS-scheduled state
    // once a memory state already exists.
    let (_, _, _, state) = card_state(&pool, user_id, card_id).await;
    assert_eq!(state.as_deref(), Some("learning"));
}

#[tokio::test]
async fn wrong_answer_after_a_review_becomes_relearning() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let card_id = 1;
    let target = target_for(&pool, card_id).await;

    // First review: correct, establishes a memory state.
    let _ = check_answer(
        State(pool.clone()),
        AppPath(card_id),
        AuthUser(user_id),
        AppJson(CheckRequest {
            answer: target,
            attempt_token: crate::cards::issue_attempt_token(user_id, card_id, 1, 0, 0).unwrap(),
        }),
    )
    .await
    .unwrap();

    // Second review: wrong, with an existing memory state - should
    // demote to "relearning", not stay "learning".
    let response = check_answer(
        State(pool.clone()),
        AppPath(card_id),
        AuthUser(user_id),
        AppJson(CheckRequest {
            answer: "definitely wrong".to_string(),
            attempt_token: crate::cards::issue_attempt_token(user_id, card_id, 1, 1, 0).unwrap(),
        }),
    )
    .await
    .unwrap();

    assert!(!response.0.correct);

    let (_, _, _, state) = card_state(&pool, user_id, card_id).await;
    assert_eq!(state.as_deref(), Some("relearning"));

    let review_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM review_history WHERE user_id = ? AND card_id = ?")
            .bind(user_id)
            .bind(card_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(review_count, 2);
}

#[tokio::test]
async fn existing_row_with_no_last_review_is_treated_as_a_new_card() {
    // `cards_states.stability`/`difficulty` are `NOT NULL DEFAULT 0` in
    // the schema (only `last_review` is nullable) - so despite
    // `check_answer`'s comment describing this branch as "FSRS state is
    // NULL", the tuple match it guards with can only ever fail via
    // `last_review` being NULL; `stability`/`difficulty` are always
    // `Some`. This simulates that real shape: a `cards_states` row that
    // exists (e.g. pre-created by some other flow) but has never
    // actually been reviewed.
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let card_id = 1;
    let target = target_for(&pool, card_id).await;

    sqlx::query(
        "INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state) VALUES (?, ?, 0, 0, NULL, 'new')",
    )
    .bind(user_id)
    .bind(card_id)
    .execute(&pool)
    .await
    .unwrap();

    let response = check_answer(
        State(pool.clone()),
        AppPath(card_id),
        AuthUser(user_id),
        AppJson(CheckRequest {
            answer: target,
            attempt_token: crate::cards::issue_attempt_token(user_id, card_id, 1, 0, 0).unwrap(),
        }),
    )
    .await
    .unwrap();

    assert!(response.0.correct);

    let (stability, _, _, state) = card_state(&pool, user_id, card_id).await;
    assert_eq!(state.as_deref(), Some("learning"));
    assert!(stability.is_some());
}

async fn revision_for(pool: &SqlitePool, user_id: i64, card_id: i64) -> i64 {
    sqlx::query_scalar("SELECT review_revision FROM cards_states WHERE user_id = ? AND card_id = ?")
        .bind(user_id)
        .bind(card_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn stale_answer_preserves_state_and_history() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let answer = target_for(&pool, 1).await;
    let _ = check_answer(
        State(pool.clone()),
        AppPath(1),
        AuthUser(user_id),
        AppJson(CheckRequest {
            answer,
            attempt_token: crate::cards::issue_attempt_token(user_id, 1, 1, 0, 0).unwrap(),
        }),
    )
    .await
    .unwrap();
    let before = card_state(&pool, user_id, 1).await;
    let result = check_answer(
        State(pool.clone()),
        AppPath(1),
        AuthUser(user_id),
        AppJson(CheckRequest {
            answer: "wrong".into(),
            attempt_token: crate::cards::issue_attempt_token(user_id, 1, 1, 0, 0).unwrap(),
        }),
    )
    .await;
    assert!(matches!(result, Err(AppError::StaleReview)));
    assert_eq!(card_state(&pool, user_id, 1).await, before);
    assert_eq!(revision_for(&pool, user_id, 1).await, 1);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM review_history WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn edit_rejects_old_prompt_without_recording_a_review() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let old_answer = target_for(&pool, 1).await;
    let edit: crate::admin::UpdateCardRequest = serde_json::from_value(
        serde_json::json!({"expected_content_revision": 0, "sentence_id": 1, "sentence": "새 답입니다.", "target": "답"}),
    )
    .unwrap();
    let edited = crate::admin::edit_card(
        crate::auth::AdminUser(user_id),
        State(pool.clone()),
        AppPath(1),
        AppJson(edit),
    )
    .await
    .unwrap();
    assert_eq!(edited.0.content_revision, 1);

    let stale = check_answer(
        State(pool.clone()),
        AppPath(1),
        AuthUser(user_id),
        AppJson(CheckRequest {
            answer: old_answer,
            attempt_token: crate::cards::issue_attempt_token(user_id, 1, 1, 0, 0).unwrap(),
        }),
    )
    .await;
    assert!(matches!(stale, Err(AppError::StaleContent)));
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM review_history WHERE user_id = ? AND card_id = 1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    let state_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM cards_states WHERE user_id = ? AND card_id = 1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state_count, 0);

    let fresh = check_answer(
        State(pool.clone()),
        AppPath(1),
        AuthUser(user_id),
        AppJson(CheckRequest {
            answer: "답".into(),
            attempt_token: crate::cards::issue_attempt_token(
                user_id,
                1,
                1,
                0,
                edited.0.content_revision,
            )
            .unwrap(),
        }),
    )
    .await
    .unwrap();
    assert!(fresh.0.correct);
    assert_eq!(fresh.0.reveal.back.target, "답");
}

#[tokio::test]
async fn grading_rejects_a_sentence_the_backend_did_not_select() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let unselected_sentence =
        sqlx::query("INSERT INTO sentences (card_id, text) VALUES (1, '새 답입니다.')")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();
    sqlx::query("INSERT INTO targets (sentence_id, form) VALUES (?, '답')")
        .bind(unselected_sentence)
        .execute(&pool)
        .await
        .unwrap();
    let other_sentence: i64 =
        sqlx::query_scalar("SELECT id FROM sentences WHERE card_id = 2 ORDER BY id LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    for sentence_id in [unselected_sentence, other_sentence] {
        let result = check_answer(
            State(pool.clone()),
            AppPath(1),
            AuthUser(user_id),
            AppJson(CheckRequest {
                answer: "anything".into(),
                attempt_token: crate::cards::issue_attempt_token(user_id, 1, sentence_id, 0, 0)
                    .unwrap(),
            }),
        )
        .await;
        assert!(matches!(result, Err(AppError::StalePrompt)));
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM review_history WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn simultaneous_answers_only_record_one_review_per_revision() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let answer = target_for(&pool, 1).await;
    // Exercise both an absent state row and the existing row it creates.
    for revision in 0..2 {
        let submit = || {
            check_answer(
                State(pool.clone()),
                AppPath(1),
                AuthUser(user_id),
                AppJson(CheckRequest {
                    answer: answer.clone(),
                    attempt_token: crate::cards::issue_attempt_token(user_id, 1, 1, revision, 0)
                        .unwrap(),
                }),
            )
        };
        let (first, second) = tokio::join!(submit(), submit());
        assert!(matches!(
            (&first, &second),
            (Ok(_), Err(AppError::StaleReview)) | (Err(AppError::StaleReview), Ok(_))
        ));
        assert_eq!(revision_for(&pool, user_id, 1).await, revision + 1);
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM review_history WHERE user_id = ?")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, revision + 1);
    }
}

#[tokio::test]
async fn missing_revision_is_a_bad_request_and_stale_errors_have_codes() {
    use axum::{
        body::{Body, to_bytes},
        extract::FromRequest,
        http::{Request, StatusCode},
        response::IntoResponse,
    };
    let request = Request::builder()
        .header("content-type", "application/json")
        .body(Body::from(r#"{"answer":"test"}"#))
        .unwrap();
    let parsed = AppJson::<CheckRequest>::from_request(request, &()).await;
    assert!(matches!(parsed, Err(AppError::BadRequest(_))));
    let request = Request::builder()
        .header("content-type", "application/json")
        .body(Body::from(r#"{"answer":"test","review_revision":0}"#))
        .unwrap();
    let parsed = AppJson::<CheckRequest>::from_request(request, &()).await;
    assert!(matches!(parsed, Err(AppError::BadRequest(_))));
    let request = Request::builder()
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"answer":"test","review_revision":0,"content_revision":0}"#,
        ))
        .unwrap();
    let parsed = AppJson::<CheckRequest>::from_request(request, &()).await;
    assert!(matches!(parsed, Err(AppError::BadRequest(_))));
    let response = AppError::StaleReview.into_response();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["code"], "stale_review");
    let response = AppError::StaleContent.into_response();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["code"], "stale_content");
    let response = AppError::StalePrompt.into_response();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["code"], "stale_prompt");
}
