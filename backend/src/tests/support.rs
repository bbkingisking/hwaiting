//! Shared harness for tier-2 tests: an in-process, fully-migrated, seeded
//! SQLite database, plus the couple of fixtures every handler-level test
//! needs (a user row to act as). `#[cfg(test)]`-only, wired in from
//! `main.rs` the same way every other module is.
//!
//! Deliberately a real temp *file* per test, not `sqlite::memory:` - a
//! multi-connection pool against a bare in-memory URI hands each connection
//! its own independent, empty database (there's nothing to share it by),
//! which would silently break any test that runs two queries concurrently
//! against the same pool. This mirrors `db::init`'s own connection setup
//! for the same reason. Each test gets its own uniquely-named file under
//! the system temp directory; nothing here deletes it afterward (SQLite's
//! `Pool`/`Connection` drop isn't synchronous, so there's no clean point to
//! hook that from without adding real complexity for a cleanup that's purely
//! cosmetic - stray files under the OS temp dir, not a correctness issue).

use sqlx::SqlitePool;

/// A fresh database with every migration applied and the 50 sample cards
/// seeded (ids 1-50), exactly what `db::init` does at real startup minus
/// the admin-user seed (tests that need an admin should insert one
/// directly - see `test_admin_user`).
pub(crate) async fn test_pool() -> SqlitePool {
    let path = std::env::temp_dir().join(format!(
        "hwaiting_test_{}_{}.db",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));

    let pool = crate::db::connect_migrated(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql(include_str!("../../seed/sample_cards.sql"))
        .execute(&mut *tx)
        .await
        .unwrap();

    tx.commit().await.unwrap();

    pool
}

/// Inserts a plain (non-admin) user with a unique throwaway username and
/// returns its id. `password_hash` is left NULL (a passkey-only account, in
/// this app's own terms) since nothing in the tier-2 tests logs in through
/// the password flow - they construct `AuthUser`/`AdminUser` directly rather
/// than going through a real HTTP request.
pub(crate) async fn test_user(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("INSERT INTO users (username, is_admin) VALUES (?, 0) RETURNING id")
        .bind(format!("test-user-{}", uuid::Uuid::new_v4()))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Same as `test_user`, but `is_admin = 1`.
pub(crate) async fn test_admin_user(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("INSERT INTO users (username, is_admin) VALUES (?, 1) RETURNING id")
        .bind(format!("test-admin-{}", uuid::Uuid::new_v4()))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[cfg(test)]
#[path = "test_support.rs"]
mod tests;

/// Add a complete prompt atomically; a sentence without a target or a translation-language
/// translation cannot be committed under the content schema.
pub(crate) async fn translated_sentence(
    pool: &SqlitePool,
    card_id: i64,
    text: &str,
    target: &str,
    translation: &str,
) -> i64 {
    let mut tx = pool.begin().await.unwrap();
    let language_id = crate::enum_lookup::eng_language_id(&mut *tx).await.unwrap();
    let sentence_id =
        sqlx::query("INSERT INTO sentences (card_id, text, required_language_id) VALUES (?, ?, ?)")
            .bind(card_id)
            .bind(text)
            .bind(language_id)
            .execute(&mut *tx)
            .await
            .unwrap()
            .last_insert_rowid();
    sqlx::query("INSERT INTO targets (sentence_id, form) VALUES (?, ?)")
        .bind(sentence_id)
        .bind(target)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sentences_translations (sentence_id, card_id, language_id, translation) VALUES (?, ?, ?, ?)")
        .bind(sentence_id).bind(card_id).bind(language_id).bind(translation).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    sentence_id
}
