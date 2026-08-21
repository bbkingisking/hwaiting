use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;
use std::env;
use std::time::Duration;
use tracing::{debug, info};

pub async fn init() -> anyhow::Result<SqlitePool> {
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL environment variable must be set");

    // WAL lets readers (GET /cards/next's "pick a due card" query, in
    // particular - it's the slowest read in the app) run concurrently with a
    // writer instead of blocking it. Without this, the pool's 5 connections
    // mean a slow read and a concurrent write (e.g. POST /cards/{id}/check,
    // which now runs earlier in a card's lifecycle than the old
    // POST /review did, right when the answer is submitted rather than at
    // Next - see cards::check_answer) contend for the same file lock, and
    // once busy_timeout (5s, sqlx's default - set explicitly below so it's
    // not just an assumption) is exhausted, every connection in the pool
    // starts failing with "database is locked", not just the two involved.
    // The rest of this codebase was written assuming FK constraints aren't
    // enforced (see e.g. moderation::comment_on_card's "check by hand so a
    // bad card_id 404s instead of silently inserting an orphaned row" -
    // manual existence checks stand in for FK enforcement throughout) - but
    // sqlx's SqliteConnectOptions defaults `foreign_keys` to `ON`, and
    // nothing here ever overrode it. That mismatch went unnoticed until
    // migration 20240101000041's `parts_of_speech` rebuild (DROP + recreate
    // + rename, needed to work around an ALTER TABLE DROP COLUMN quirk)
    // failed with "FOREIGN KEY constraint failed": SQLite's DROP TABLE,
    // under FK enforcement, does an implicit delete-every-row first, which
    // trips every un-cascaded FK still pointing at that table (cards.pos_id
    // among them). Disabling it here makes actual behavior match what the
    // rest of the code already assumed.
    let options = SqliteConnectOptions::from_str(&database_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(false)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(200)
        .connect_with(options)
        .await?;

    // Run migrations
    debug!("Running database migrations...");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await?;
    debug!("Database migrations complete");

    // Seed admin user if doesn't exist
    seed_admin_user(&pool).await?;

    // Seed sample cards if doesn't exist
    seed_sample_cards(&pool).await?;

    Ok(pool)
}

async fn seed_admin_user(pool: &SqlitePool) -> anyhow::Result<Option<i64>> {
    use argon2::{
        password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
        Argon2,
    };

    // Get admin credentials from environment or systemd credential store
    let admin_username = env::var("ADMIN_USERNAME")
        .expect("ADMIN_USERNAME environment variable must be set");
    let admin_password = crate::credentials::admin_password();

    // Check if admin user already exists
    let admin_exists: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM users WHERE username = ?"
    )
    .bind(&admin_username)
    .fetch_optional(pool)
    .await?;

    if let Some(_existing_id) = admin_exists {
        debug!("Admin user already exists, skipping seed");
        return Ok(None);
    }

    info!("Creating admin user: {}", admin_username);

    // Hash the password
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(admin_password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?
        .to_string();

    // Create admin user
    sqlx::query(
        "INSERT INTO users (username, password_hash, is_admin) VALUES (?, ?, 1)"
    )
    .bind(&admin_username)
    .bind(&password_hash)
    .execute(pool)
    .await?;

    let user_id: i64 = sqlx::query_scalar(
        "SELECT id FROM users WHERE username = ?"
    )
    .bind(&admin_username)
    .fetch_one(pool)
    .await?;

    info!("Admin user created successfully");

    Ok(Some(user_id))
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
