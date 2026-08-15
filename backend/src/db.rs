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
    let options = SqliteConnectOptions::from_str(&database_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
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
