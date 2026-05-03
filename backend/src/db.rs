use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;
use std::env;
use tracing::info;
use std::fs;
use std::path::Path;
use serde::Deserialize;

#[derive(Deserialize)]
struct DeckManifestEntry {
    language_code: String,
    language_name: String,
    icon: String,
    file: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeckFile {
    cards: Vec<Card>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Card {
    form: String,
    #[allow(dead_code)]
    lemma: Option<String>,
    pos: Option<String>,
    grammatical_note: Option<String>,
    translation: String,
    example: String,
    example_translation: Option<String>,
}

fn read_manifest() -> anyhow::Result<Vec<DeckManifestEntry>> {
    let manifest_path = Path::new("./decks/manifest.json");
    if !manifest_path.exists() {
        info!("No manifest.json found in decks directory");
        return Ok(vec![]);
    }
    let contents = fs::read_to_string(manifest_path)?;
    let manifest: Vec<DeckManifestEntry> = serde_json::from_str(&contents)?;
    Ok(manifest)
}

pub async fn init() -> anyhow::Result<SqlitePool> {
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL environment variable must be set");

    let options = SqliteConnectOptions::from_str(&database_url)?
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await?;

    // Read deck manifest
    let manifest = read_manifest()?;

    // Seed admin user if doesn't exist
    seed_admin_user(&pool, &manifest).await?;

    // Seed decks from manifest
    seed_decks_from_manifest(&pool, &manifest).await?;

    Ok(pool)
}

async fn seed_admin_user(pool: &SqlitePool, manifest: &[DeckManifestEntry]) -> anyhow::Result<Option<i64>> {
    use argon2::{
        password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
        Argon2,
    };

    // Get admin credentials from environment variables
    let admin_username = env::var("ADMIN_USERNAME")
        .expect("ADMIN_USERNAME environment variable must be set");
    let admin_password = env::var("ADMIN_PASSWORD")
        .expect("ADMIN_PASSWORD environment variable must be set");

    // Check if admin user already exists
    let admin_exists: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM users WHERE username = ?"
    )
    .bind(&admin_username)
    .fetch_optional(pool)
    .await?;

    if let Some(_existing_id) = admin_exists {
        info!("Admin user already exists, skipping seed");
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

    // Use first manifest entry as default target language
    let default_language_id: Option<i64> = if let Some(entry) = manifest.first() {
        sqlx::query_scalar("SELECT id FROM languages WHERE code = ?")
            .bind(&entry.language_code)
            .fetch_optional(pool)
            .await?
    } else {
        None
    };

    // Create admin user with default target language
    sqlx::query(
        "INSERT INTO users (username, password_hash, is_admin, target_language_id) VALUES (?, ?, 1, ?)"
    )
    .bind(&admin_username)
    .bind(&password_hash)
    .bind(default_language_id)
    .execute(pool)
    .await?;

    let user_id: i64 = sqlx::query_scalar(
        "SELECT id FROM users WHERE username = ?"
    )
    .bind(&admin_username)
    .fetch_one(pool)
    .await?;

    if let Some(entry) = manifest.first() {
        info!("Admin user created successfully with {} as target language", entry.language_name);
    } else {
        info!("Admin user created successfully (no manifest entries for default language)");
    }

    Ok(Some(user_id))
}

async fn seed_decks_from_manifest(pool: &SqlitePool, manifest: &[DeckManifestEntry]) -> anyhow::Result<()> {
    if manifest.is_empty() {
        info!("No deck manifest entries found, skipping word seeding");
        return Ok(());
    }

    let decks_path = Path::new("./decks");
    let mut total_words = 0;

    for entry in manifest {
        info!("Processing deck: {} ({})", entry.language_name, entry.language_code);

        // Upsert language (insert if not exists, update icon)
        sqlx::query(
            "INSERT INTO languages (code, name, icon) VALUES (?, ?, ?)
             ON CONFLICT(code) DO UPDATE SET name = excluded.name, icon = excluded.icon"
        )
        .bind(&entry.language_code)
        .bind(&entry.language_name)
        .bind(&entry.icon)
        .execute(pool)
        .await?;

        // Get language ID
        let language_id: i64 = sqlx::query_scalar(
            "SELECT id FROM languages WHERE code = ?"
        )
        .bind(&entry.language_code)
        .fetch_one(pool)
        .await?;

        // Read the deck file
        let file_path = decks_path.join(&entry.file);
        if !file_path.exists() {
            info!("Deck file {} not found, skipping", entry.file);
            continue;
        }

        let contents = fs::read_to_string(&file_path)?;
        let deck: DeckFile = serde_json::from_str(&contents)?;

        let mut deck_word_count = 0;

        for card in &deck.cards {
            // Skip if word already exists
            let existing: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM words WHERE form = ? AND language_id = ?"
            )
            .bind(&card.form)
            .bind(language_id)
            .fetch_optional(pool)
            .await?;

            if existing.is_some() {
                continue;
            }

            let hint = &card.translation;
            let context_text = &card.example;
            let context_translation = card.example_translation.as_deref().unwrap_or("");
            let grammar = card.pos.as_deref();
            let politeness = card.grammatical_note.as_deref();
            let notes_json = "[]";

            // Insert word with language_id (no review history)
            sqlx::query(
                "INSERT INTO words (form, hint, context, context_translation, grammar, politeness, notes, language_id)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&card.form)
            .bind(hint)
            .bind(context_text)
            .bind(context_translation)
            .bind(grammar)
            .bind(politeness)
            .bind(notes_json)
            .bind(language_id)
            .execute(pool)
            .await?;

            deck_word_count += 1;
            total_words += 1;
        }

        info!("Seeded {} new words for {}", deck_word_count, entry.language_name);
    }

    if total_words > 0 {
        info!("Total new words seeded across all decks: {}", total_words);
    }

    let final_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM words")
        .fetch_one(pool)
        .await?;

    info!("Total words in database: {}", final_count);

    Ok(())
}

