use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, info};

pub async fn init() -> anyhow::Result<SqlitePool> {
    let database_url = crate::credentials::database_url();
    let pool = connect_migrated(&database_url).await?;
    crate::cards::scheduling::backfill_due_dates(&pool).await?;
    debug!("Database migrations complete");

    // Seed admin user if doesn't exist
    seed_admin_user(&pool).await?;

    // Seed sample cards if doesn't exist
    seed_sample_cards(&pool).await?;

    Ok(pool)
}

/// Table rebuild migrations need foreign keys disabled while they drop and
/// replace referenced tables. Keep that exception on a dedicated migration
/// pool; every connection returned to the application enforces relationships.
pub(crate) async fn connect_migrated(database_url: &str) -> anyhow::Result<SqlitePool> {
    // WAL lets readers (GET /cards/queue's "pick a due card" query, in
    // particular - it's the slowest read in the app) run concurrently with a
    // writer instead of blocking it. Without this, the pool's 5 connections
    // mean a slow read and a concurrent write (e.g. POST /cards/{id}/check,
    // which now runs earlier in a card's lifecycle than the old
    // POST /review did, right when the answer is submitted rather than at
    // Next - see cards::check_answer) contend for the same file lock, and
    // once busy_timeout (5s, sqlx's default - set explicitly below so it's
    // not just an assumption) is exhausted, every connection in the pool
    // starts failing with "database is locked", not just the two involved.
    let options = SqliteConnectOptions::from_str(&database_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(false)
        .busy_timeout(Duration::from_secs(5));

    let migration_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;

    debug!("Running database migrations...");
    sqlx::migrate!("./migrations").run(&migration_pool).await?;

    // Existing installations may contain orphaned rows written while FK
    // enforcement was disabled. Fail with the offending relationship rather
    // than silently treating that data as valid after the policy change.
    if let Some(row) = sqlx::query("PRAGMA foreign_key_check")
        .fetch_optional(&migration_pool)
        .await?
    {
        let table: String = row.get("table");
        let rowid: Option<i64> = row.get("rowid");
        let parent: String = row.get("parent");
        anyhow::bail!(
            "Database has a foreign-key violation: {table} row {rowid:?} references missing {parent}; repair it before starting the application"
        );
    }
    migration_pool.close().await;

    let runtime_options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(runtime_options)
        .await?;

    Ok(pool)
}

async fn seed_admin_user(pool: &SqlitePool) -> anyhow::Result<()> {
    use argon2::{
        Argon2,
        password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
    };

    // Get admin credentials from environment or systemd credential store
    let admin_username = crate::credentials::admin_username();
    let admin_password = crate::credentials::admin_password();

    // Check if admin user already exists
    let admin_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE username = ?)")
            .bind(&admin_username)
            .fetch_one(pool)
            .await?;

    if admin_exists {
        debug!("Admin user already exists, skipping seed");
        return Ok(());
    }

    info!("Creating admin user: {}", admin_username);

    // Hash the password
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(admin_password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?
        .to_string();

    sqlx::query("INSERT INTO users (username, password_hash, is_admin) VALUES (?, ?, 1)")
        .bind(&admin_username)
        .bind(&password_hash)
        .execute(pool)
        .await?;

    info!("Admin user created successfully");

    Ok(())
}

/// Seeds the 50 sample cards (frequency_rank 1-50, ids 1-50) a fresh/demo
/// deployment needs to have real review content out of the box. Kept out
/// of migrations deliberately -- see backend/seed/sample_cards.sql's own
/// header and the migration this replaced (the old 015/016 seed migrations
/// baked in data that turned out wrong and incomplete once the conjugation
/// matrix existed, and a migration can't cleanly be regenerated/replaced
/// once applied). This is content, not schema.
///
/// The "if ids 1-50 don't exist yet" check is trustworthy here in a way it
/// wouldn't have been with the old migrations still in the chain: nothing
/// else ever writes cards with those ids except real card data (which must
/// never be touched) or this function itself, so seeing id 1 already
/// present unambiguously means "don't seed" either way.
async fn seed_sample_cards(pool: &SqlitePool) -> anyhow::Result<()> {
    let already_seeded: Option<i64> = sqlx::query_scalar("SELECT id FROM cards WHERE id = 1")
        .fetch_optional(pool)
        .await?;

    if already_seeded.is_some() {
        debug!("Sample cards already present, skipping seed");
        return Ok(());
    }

    info!("Seeding 50 sample cards");
    sqlx::raw_sql(include_str!("../seed/sample_cards.sql"))
        .execute(pool)
        .await?;
    info!("Sample cards seeded successfully");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_pool;

    #[tokio::test]
    async fn every_runtime_connection_enforces_foreign_keys() {
        let pool = test_pool().await;
        let mut connections = Vec::new();
        for _ in 0..5 {
            connections.push(pool.acquire().await.unwrap());
        }
        for connection in &mut connections {
            let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
                .fetch_one(&mut **connection)
                .await
                .unwrap();
            assert_eq!(enabled, 1);
        }
        drop(connections);

        let result = sqlx::query(
            "INSERT INTO users_card_flags (user_id, card_id, suppressed) VALUES (999999, 999999, 1)",
        )
        .execute(&pool)
        .await;
        assert!(
            result.is_err(),
            "an orphaned flag must be rejected by SQLite"
        );
    }

    #[tokio::test]
    async fn existing_foreign_key_violations_block_startup() {
        let path = std::env::temp_dir().join(format!(
            "hwaiting_fk_test_{}_{}.db",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let database_url = format!("sqlite://{}", path.display());
        connect_migrated(&database_url).await.unwrap().close().await;

        // Simulate a row written by an older application connection with
        // foreign keys disabled, then verify the next startup catches it.
        let old_options = SqliteConnectOptions::from_str(&database_url)
            .unwrap()
            .foreign_keys(false);
        let old_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(old_options)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO users_card_flags (user_id, card_id, suppressed) VALUES (999999, 999999, 1)",
        )
        .execute(&old_pool)
        .await
        .unwrap();
        old_pool.close().await;

        let error = match connect_migrated(&database_url).await {
            Ok(_) => panic!("startup should reject the orphaned flag"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("users_card_flags"), "{error}");
        assert!(error.contains("missing"), "{error}");
    }
}
