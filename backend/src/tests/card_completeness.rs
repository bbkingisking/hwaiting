//! Database contracts, independent of the HTTP representation of a card.
use super::*;

const COMPLETE_CARD: &str = r#"
    INSERT INTO cards (id, word, required_sentence_id) VALUES (100001, '물', 200001);
    INSERT INTO sentences (id, card_id, text, required_language_id)
        VALUES (200001, 100001, '물을 마셔요.', 1);
    INSERT INTO targets (sentence_id, form) VALUES (200001, '물을');
    INSERT INTO cards_translations (card_id, language_id, trans_word, required_sentence_id)
        VALUES (100001, 1, 'water', 200001);
    INSERT INTO sentences_translations (sentence_id, card_id, language_id, translation)
        VALUES (200001, 100001, 1, 'I drink water.');
"#;

const SECOND_SENTENCE: &str = r#"
    INSERT INTO sentences (id, card_id, text, required_language_id)
        VALUES (200002, 100001, '물이 차가워요.', 1);
    INSERT INTO targets (sentence_id, form) VALUES (200002, '물이');
    INSERT INTO sentences_translations (sentence_id, card_id, language_id, translation)
        VALUES (200002, 100001, 1, 'The water is cold.');
"#;

const FRENCH_TRANSLATION: &str = r#"
    INSERT INTO languages (id, slug) VALUES (3, 'fra');
    INSERT INTO cards_translations (card_id, language_id, trans_word, required_sentence_id)
        VALUES (100001, 3, 'eau', 200001);
    INSERT INTO sentences_translations (sentence_id, card_id, language_id, translation)
        VALUES (200001, 100001, 3, 'Je bois de l’eau.');
"#;

async fn empty_pool() -> SqlitePool {
    let path = std::env::temp_dir().join(format!("hwaiting_complete_{}.db", uuid::Uuid::new_v4()));
    connect_migrated(&format!("sqlite://{}", path.display()))
        .await
        .unwrap()
}

async fn commit_sql(pool: &SqlitePool, sql: &str) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql(sql).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

async fn reject_commit(pool: &SqlitePool, sql: &str) {
    let mut connection = pool.acquire().await.unwrap();
    sqlx::query("BEGIN")
        .execute(&mut *connection)
        .await
        .unwrap();
    // The intermediate state is allowed; the incomplete COMMIT must fail.
    sqlx::raw_sql(sql)
        .execute(&mut *connection)
        .await
        .unwrap_or_else(|error| panic!("statement failed before COMMIT: {sql}\n{error}"));
    let error = sqlx::query("COMMIT")
        .execute(&mut *connection)
        .await
        .expect_err(sql);
    assert!(
        error
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation(),
        "{sql}: {error}"
    );
    sqlx::query("ROLLBACK")
        .execute(&mut *connection)
        .await
        .unwrap();
}

#[tokio::test]
async fn complete_card_commits_in_either_insertion_order() {
    let pool = empty_pool().await;
    commit_sql(&pool, COMPLETE_CARD).await;
    commit_sql(&pool, "DELETE FROM cards WHERE id = 100001").await;
    // No row order is necessary for the cyclic content relationships.
    let reversed = COMPLETE_CARD
        .split(';')
        .filter(|statement| !statement.trim().is_empty())
        .rev()
        .map(|statement| format!("{statement};"))
        .collect::<String>();
    commit_sql(&pool, &reversed).await;
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn each_required_part_is_checked_at_commit() {
    let pool = empty_pool().await;
    for missing_table in [
        "cards",
        "sentences",
        "targets",
        "cards_translations",
        "sentences_translations",
    ] {
        let incomplete = COMPLETE_CARD
            .split(';')
            .filter(|statement| !statement.trim().is_empty())
            .filter(|statement| {
                !statement
                    .trim()
                    .starts_with(&format!("INSERT INTO {missing_table} ("))
            })
            .map(|statement| format!("{statement};"))
            .collect::<String>();
        reject_commit(&pool, &incomplete).await;
    }
}

#[tokio::test]
async fn language_registration_requires_no_content_and_coverage_can_differ() {
    let pool = empty_pool().await;
    commit_sql(&pool, COMPLETE_CARD).await;
    commit_sql(&pool, "INSERT INTO languages (id, slug) VALUES (4, 'deu')").await;
    commit_sql(&pool, FRENCH_TRANSLATION).await;
    let coverage: Vec<(String, i64)> = sqlx::query_as(
        "SELECT l.slug, count(*) FROM sentences_translations st JOIN languages l ON l.id = st.language_id GROUP BY l.slug ORDER BY l.slug",
    ).fetch_all(&pool).await.unwrap();
    assert_eq!(coverage, vec![("eng".into(), 1), ("fra".into(), 1)]);
}

#[tokio::test]
async fn neither_translation_half_can_be_added_alone() {
    let pool = empty_pool().await;
    commit_sql(&pool, COMPLETE_CARD).await;
    commit_sql(&pool, "INSERT INTO languages (id, slug) VALUES (3, 'fra')").await;
    reject_commit(&pool, "INSERT INTO cards_translations (card_id, language_id, trans_word, required_sentence_id) VALUES (100001, 3, 'eau', 200001)").await;
    reject_commit(&pool, "INSERT INTO sentences_translations (sentence_id, card_id, language_id, translation) VALUES (200001, 100001, 3, 'Je bois de l’eau.')").await;
}

#[tokio::test]
async fn a_card_cannot_have_a_second_sentence() {
    let pool = empty_pool().await;
    commit_sql(&pool, COMPLETE_CARD).await;
    let error = sqlx::raw_sql(SECOND_SENTENCE)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(error.as_database_error().unwrap().is_unique_violation());
}

#[tokio::test]
async fn dictionary_entries_are_unique_but_homonyms_and_manual_cards_are_allowed() {
    let pool = empty_pool().await;
    commit_sql(&pool, COMPLETE_CARD).await;
    let homonym = COMPLETE_CARD
        .replace("100001", "100002")
        .replace("200001", "200002");
    commit_sql(&pool, &homonym).await;
    commit_sql(&pool, "UPDATE cards SET krdict_id = id").await;
    let error = sqlx::query("UPDATE cards SET krdict_id = 100001 WHERE id = 100002")
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(error.as_database_error().unwrap().is_unique_violation());
    commit_sql(&pool, "UPDATE cards SET krdict_id = NULL").await;
    let words: Vec<String> = sqlx::query_scalar("SELECT word FROM cards")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(words, vec!["물", "물"]);
}

#[tokio::test]
async fn witnesses_cannot_borrow_another_cards_content_or_language() {
    let pool = empty_pool().await;
    commit_sql(&pool, COMPLETE_CARD).await;
    let other_card = COMPLETE_CARD
        .replace("100001", "100002")
        .replace("200001", "200003");
    commit_sql(&pool, &other_card).await;
    for invalid_update in [
        "UPDATE cards SET required_sentence_id = 200003 WHERE id = 100001",
        "UPDATE cards_translations SET required_sentence_id = 200003 WHERE card_id = 100001",
        "UPDATE sentences_translations SET card_id = 100002 WHERE sentence_id = 200001",
        "UPDATE sentences SET required_language_id = 2 WHERE id = 200001",
        "UPDATE cards_translations SET language_id = 2 WHERE card_id = 100001",
        "UPDATE sentences_translations SET language_id = 2 WHERE sentence_id = 200001",
    ] {
        reject_commit(&pool, invalid_update).await;
    }
}

#[tokio::test]
async fn deletion_cannot_leave_incomplete_content() {
    let pool = empty_pool().await;
    commit_sql(&pool, COMPLETE_CARD).await;
    for table in [
        "targets",
        "sentences",
        "cards_translations",
        "sentences_translations",
    ] {
        reject_commit(&pool, &format!("DELETE FROM {table}")).await;
    }
    commit_sql(&pool, "DELETE FROM cards WHERE id = 100001").await;
    for table in [
        "cards",
        "sentences",
        "targets",
        "cards_translations",
        "sentences_translations",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "{table}");
    }
}

#[tokio::test]
async fn language_witness_can_be_repointed_when_removing_a_translation() {
    let pool = empty_pool().await;
    commit_sql(&pool, COMPLETE_CARD).await;
    commit_sql(&pool, FRENCH_TRANSLATION).await;
    commit_sql(
        &pool,
        r#"
        UPDATE sentences SET required_language_id = 3 WHERE id = 200001;
        DELETE FROM cards_translations WHERE card_id = 100001 AND language_id = 1;
        DELETE FROM sentences_translations WHERE card_id = 100001 AND language_id = 1;
    "#,
    )
    .await;
    let languages: Vec<i64> = sqlx::query_scalar("SELECT language_id FROM cards_translations")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(languages, vec![3]);
}

#[tokio::test]
async fn required_text_rejects_null_empty_and_unicode_whitespace() {
    let pool = empty_pool().await;
    commit_sql(&pool, COMPLETE_CARD).await;
    for (table, field) in [
        ("cards", "word"),
        ("sentences", "text"),
        ("targets", "form"),
        ("cards_translations", "trans_word"),
        ("sentences_translations", "translation"),
        ("cards_translations", "trans_dfn"),
    ] {
        for value in ["", " \t\r\n", "\u{a0}\u{2003}\u{3000}"] {
            let error = sqlx::query(&format!("UPDATE {table} SET {field} = ?"))
                .bind(value)
                .execute(&pool)
                .await
                .unwrap_err();
            assert!(
                error.as_database_error().unwrap().is_check_violation(),
                "{table}.{field}: {error}"
            );
        }
        if field != "trans_dfn" {
            let error = sqlx::query(&format!("UPDATE {table} SET {field} = NULL"))
                .execute(&pool)
                .await
                .unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().kind(),
                sqlx::error::ErrorKind::NotNullViolation
            );
        }
    }
    // Definition is optional; no placeholder text is necessary.
    commit_sql(&pool, "UPDATE cards_translations SET trans_dfn = NULL").await;
}

#[tokio::test]
async fn translations_remain_unique_per_entity_and_language() {
    let pool = empty_pool().await;
    commit_sql(&pool, COMPLETE_CARD).await;
    for statement in [
        "INSERT INTO cards_translations (card_id, language_id, trans_word, required_sentence_id) SELECT card_id, language_id, trans_word, required_sentence_id FROM cards_translations",
        "INSERT INTO sentences_translations (sentence_id, card_id, language_id, translation) SELECT sentence_id, card_id, language_id, translation FROM sentences_translations",
    ] {
        let error = sqlx::query(statement).execute(&pool).await.unwrap_err();
        assert!(error.as_database_error().unwrap().is_unique_violation());
    }
}

const LEGACY_CARD: &str = r#"
    INSERT INTO cards (id, word) VALUES (100001, '물');
    INSERT INTO sentences (id, card_id, text) VALUES (200001, 100001, '물을 마셔요.');
    INSERT INTO targets (sentence_id, form) VALUES (200001, '물을');
    INSERT INTO cards_translations (id, card_id, language_id, trans_word, trans_dfn)
        VALUES (300001, 100001, 1, 'water', NULL);
    INSERT INTO sentences_translations (id, sentence_id, language_id, translation)
        VALUES (400001, 200001, 1, 'I drink water.');
"#;

async fn legacy_pool() -> (String, SqlitePool) {
    let path = std::env::temp_dir().join(format!("hwaiting_upgrade_{}.db", uuid::Uuid::new_v4()));
    let url = format!("sqlite://{}", path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::from_str(&url)
                .unwrap()
                .create_if_missing(true)
                .foreign_keys(false),
        )
        .await
        .unwrap();
    let migrations = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            sqlx::migrate!("./migrations")
                .iter()
                .filter(|migration| migration.version < 20261122000001)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    migrations.run(&pool).await.unwrap();
    commit_sql(&pool, LEGACY_CARD).await;
    (url, pool)
}

#[tokio::test]
async fn upgrade_preserves_content_and_adds_matching_witnesses() {
    let (url, old_pool) = legacy_pool().await;
    commit_sql(
        &old_pool,
        r#"
        INSERT INTO languages (id, slug) VALUES (3, 'fra');
        INSERT INTO cards_translations (id, card_id, language_id, trans_word)
            VALUES (300002, 100001, 3, 'eau');
        INSERT INTO sentences_translations (id, sentence_id, language_id, translation)
            VALUES (400002, 200001, 3, 'L’eau est froide.');
    "#,
    )
    .await;
    old_pool.close().await;
    let pool = connect_migrated(&url).await.unwrap();
    let words: Vec<(i64, String, i64)> = sqlx::query_as(
        "SELECT id, trans_word, required_sentence_id FROM cards_translations ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        words,
        vec![
            (300001, "water".into(), 200001),
            (300002, "eau".into(), 200001)
        ]
    );
    let sentences: Vec<(i64, i64)> =
        sqlx::query_as("SELECT id, required_language_id FROM sentences ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(sentences, vec![(200001, 1)]);
    let translations: Vec<(i64, i64, String)> =
        sqlx::query_as("SELECT id, card_id, translation FROM sentences_translations ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        translations,
        vec![
            (400001, 100001, "I drink water.".into()),
            (400002, 100001, "L’eau est froide.".into()),
        ]
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
    pool.close().await;
    // A second startup does not rebuild or change the selected references.
    connect_migrated(&url).await.unwrap().close().await;
}

#[tokio::test]
async fn upgrade_rejects_incomplete_or_blank_legacy_data_atomically() {
    for invalid_data in [
        "DELETE FROM sentences_translations",
        "DELETE FROM cards_translations",
        "DELETE FROM targets",
        "DELETE FROM sentences",
        "UPDATE sentences SET text = '   '",
        "UPDATE cards_translations SET trans_word = char(9)",
        "INSERT INTO sentences_translations (sentence_id, language_id, translation) VALUES (999999, 1, 'orphan')",
    ] {
        let (url, old_pool) = legacy_pool().await;
        commit_sql(&old_pool, invalid_data).await;
        assert!(connect_migrated(&url).await.is_err(), "{invalid_data}");
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('cards')")
                .fetch_all(&old_pool)
                .await
                .unwrap();
        assert!(
            !columns
                .iter()
                .any(|column| column == "required_sentence_id")
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM cards WHERE id = 100001")
            .fetch_one(&old_pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        let applied: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _sqlx_migrations WHERE version = 20261122000001",
        )
        .fetch_one(&old_pool)
        .await
        .unwrap();
        assert_eq!(applied, 0);
        old_pool.close().await;
    }
}

#[tokio::test]
async fn language_codes_use_three_lowercase_letters() {
    let pool = empty_pool().await;
    for invalid in ["en", "en-US", "ENG", "french", "", "12a"] {
        let error = sqlx::query("INSERT INTO languages (slug) VALUES (?)")
            .bind(invalid)
            .execute(&pool)
            .await
            .unwrap_err();
        assert!(error.as_database_error().unwrap().is_check_violation());
    }
    commit_sql(&pool, "INSERT INTO languages (slug) VALUES ('fra')").await;
}

#[tokio::test]
async fn incomplete_autocommit_insert_is_rejected() {
    let pool = empty_pool().await;
    let error = sqlx::query(
        "INSERT INTO cards (id, word, required_sentence_id) VALUES (100001, '물', 200001)",
    )
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(
        error
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM cards")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
