use super::*;

/// Just building the harness is the test: migrations apply cleanly from
/// empty, and all 50 sample cards land. The migration-ordering /
/// foreign-key-during-DROP-TABLE breakage handled in `db::connect_migrated` and
/// the "sample cards were wrong/incomplete" fix commits both would have
/// been caught here.
#[tokio::test]
async fn migrations_and_seed_apply_cleanly() {
    let pool = test_pool().await;

    let card_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cards")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(card_count, 50);

    // Every seeded card has a translation, a sentence and a target -
    // the minimum shape check_answer/get_review_queue require to consider
    // a card reviewable at all.
    let reviewable_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM cards c
        WHERE EXISTS (SELECT 1 FROM cards_translations ct WHERE ct.card_id = c.id)
          AND EXISTS (
              SELECT 1 FROM sentences s
              JOIN targets tg ON tg.sentence_id = s.id
              WHERE s.card_id = c.id
          )
        "#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reviewable_count, 50);
}

// No standalone test for test_user/test_admin_user themselves: every
// handler-level test elsewhere in this crate depends on them working,
// so a break here would already fail loudly and everywhere else first.
