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

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;
use std::time::Duration;

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

    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
        .unwrap()
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(false)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .unwrap();

    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::raw_sql(include_str!("../seed/sample_cards.sql"))
        .execute(&pool)
        .await
        .unwrap();

    pool
}

/// Inserts a plain (non-admin) user with a unique throwaway username and
/// returns its id. `password_hash` is left NULL (a passkey-only account, in
/// this app's own terms) since nothing in the tier-2 tests logs in through
/// the password flow - they construct `AuthUser`/`AdminUser` directly rather
/// than going through a real HTTP request.
pub(crate) async fn test_user(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO users (username, is_admin) VALUES (?, 0) RETURNING id",
    )
    .bind(format!("test-user-{}", uuid::Uuid::new_v4()))
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Same as `test_user`, but `is_admin = 1`.
pub(crate) async fn test_admin_user(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO users (username, is_admin) VALUES (?, 1) RETURNING id",
    )
    .bind(format!("test-admin-{}", uuid::Uuid::new_v4()))
    .fetch_one(pool)
    .await
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Just building the harness is the test: migrations apply cleanly from
    /// empty, and all 50 sample cards land. The migration-ordering /
    /// foreign-key-during-DROP-TABLE breakage documented in `db::init` and
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
        // the minimum shape check_answer/get_next_card require to consider
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
}
