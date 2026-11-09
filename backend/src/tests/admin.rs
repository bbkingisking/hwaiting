use super::*;

// Only the fields relevant to the three-state distinction are set in
// these fixtures; the rest of `UpdateCardRequest` deserializes to `None`
// (omitted) via each field's own `Option` default, which is not what's
// under test here.

#[test]
fn key_absent_is_none() {
    let req: UpdateCardRequest = serde_json::from_str(r#"{"sentence_id":1}"#).unwrap();
    assert!(req.hanja.is_none());
}

#[test]
fn key_explicit_null_is_some_none() {
    // This is the case the hanja-edit-not-persisting bug got wrong:
    // an explicit `null` (clear the column) used to be indistinguishable
    // from the key being absent (leave the column alone).
    let req: UpdateCardRequest =
        serde_json::from_str(r#"{"sentence_id":1,"hanja": null}"#).unwrap();
    assert_eq!(req.hanja, Some(None));
}

#[test]
fn key_with_value_is_some_some() {
    let req: UpdateCardRequest =
        serde_json::from_str(r#"{"sentence_id":1,"hanja": "漢字"}"#).unwrap();
    assert_eq!(req.hanja, Some(Some("漢字".to_string())));
}

// No separate bool-field or plain-Option-field variants: `double_option`
// is generic over `T`, and a plain `Option<T>` field is just derived
// serde with no custom code of ours behind it - neither would exercise
// anything the three cases above don't already cover.

// --- edit_card (handler-level, in-process SQLite) -------------------------

use crate::test_support::{test_admin_user, test_pool};

async fn sentence_and_target(pool: &SqlitePool, card_id: i64) -> (i64, String, String) {
    sqlx::query_as(
        "SELECT s.id, s.text, tg.form FROM sentences s JOIN targets tg ON tg.sentence_id = s.id WHERE s.card_id = ?",
    )
    .bind(card_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn target_not_in_existing_sentence_is_rejected() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;
    let card_id = 1;

    let result = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(card_id),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            target: Some("절대로존재하지않는단어".to_string()),
            ..Default::default()
        }),
    )
    .await;

    assert!(matches!(result, Err(AppError::BadRequest(_))));
}

#[tokio::test]
async fn new_sentence_missing_the_existing_target_is_rejected() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;
    let card_id = 1;

    let result = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(card_id),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            sentence: Some("이 문장에는 정답이 전혀 없습니다".to_string()),
            ..Default::default()
        }),
    )
    .await;

    assert!(matches!(result, Err(AppError::BadRequest(_))));
}

#[tokio::test]
async fn sentence_update_is_validated_against_the_unchanged_target() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;
    let card_id = 1;
    let (sentence_id, _old_sentence, target) = sentence_and_target(&pool, card_id).await;
    let new_sentence = format!("{} 그리고 다른 문장입니다", target);

    let result = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(card_id),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            sentence: Some(new_sentence.clone()),
            ..Default::default()
        }),
    )
    .await;
    let result = result.unwrap().0;
    assert_eq!(result.card.front.sentence_id, sentence_id);
    assert_eq!(result.card.back.sentence, new_sentence);
    assert_eq!(result.card.back.target, target);
    assert_eq!(result.card.front.sentence_before, "");
    assert_eq!(result.card.front.sentence_after, " 그리고 다른 문장입니다");

    let stored: String = sqlx::query_scalar("SELECT text FROM sentences WHERE id = ?")
        .bind(sentence_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, new_sentence);
}

#[tokio::test]
async fn edit_returns_the_same_canonical_projection_as_search() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;
    let response = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(1),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            sentence: Some("새 답입니다.".into()),
            target: Some("답".into()),
            is_honorific: Some(Some(true)),
            is_humble: Some(Some(true)),
            ..Default::default()
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(response.card.front.sentence_before, "새 ");
    assert_eq!(response.card.front.sentence_after, "입니다.");
    assert!(response.card.front.inflection_hint.is_honorific);
    assert!(response.card.front.inflection_hint.is_humble);
    let search = search_cards(
        AdminUser(admin_id),
        State(pool),
        AppQuery(SearchCardsQuery { q: "1".into() }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(search.cards.len(), 1);
    assert_eq!(
        serde_json::to_value(response.card).unwrap(),
        serde_json::to_value(&search.cards[0]).unwrap()
    );
}

#[tokio::test]
async fn alternatives_are_trimmed_and_blanks_dropped() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;
    let card_id = 1;
    let (sentence_id, _, _) = sentence_and_target(&pool, card_id).await;

    let result = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(card_id),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            alternatives: Some(vec![
                "  alt1  ".to_string(),
                "".to_string(),
                "   ".to_string(),
                "alt2".to_string(),
            ]),
            ..Default::default()
        }),
    )
    .await;
    assert!(result.is_ok());

    let mut stored: Vec<String> = sqlx::query_scalar(
        "SELECT alt_target FROM targets_alternatives WHERE sentence_id = ? ORDER BY alt_target",
    )
    .bind(sentence_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    stored.sort();
    assert_eq!(stored, vec!["alt1".to_string(), "alt2".to_string()]);
}

#[tokio::test]
async fn editing_a_nonexistent_card_is_not_found() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;

    let result = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(999999),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            word: Some("x".to_string()),
            ..Default::default()
        }),
    )
    .await;

    assert!(matches!(result, Err(AppError::NotFound)));
}

#[tokio::test]
async fn editing_a_second_sentence_does_not_change_the_first() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;
    let first: (i64, String) = sqlx::query_as("SELECT id, text FROM sentences WHERE card_id = 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let second = sqlx::query("INSERT INTO sentences (card_id, text) VALUES (1, '새 답입니다.')")
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_rowid();
    sqlx::query("INSERT INTO targets (sentence_id, form) VALUES (?, '답')")
        .bind(second)
        .execute(&pool)
        .await
        .unwrap();
    let _ = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(1),
        AppJson(UpdateCardRequest {
            sentence_id: second,
            sentence: Some("새 답도 있습니다.".into()),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, text FROM sentences WHERE card_id = 1 ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(rows, vec![first, (second, "새 답도 있습니다.".into())]);

    let result = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(2),
        AppJson(UpdateCardRequest {
            sentence_id: second,
            word: Some("wrong card".into()),
            ..Default::default()
        }),
    )
    .await;
    assert!(matches!(result, Err(AppError::NotFound)));
}
