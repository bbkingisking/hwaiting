use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;
use std::env;
use tracing::info;
use std::fs;
use std::path::Path;

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

    // Seed admin user if doesn't exist
    seed_admin_user(&pool).await?;

    // Seed words if table is empty
    seed_words_if_empty(&pool).await?;



    Ok(pool)
}

async fn seed_admin_user(pool: &SqlitePool) -> anyhow::Result<Option<i64>> {
    use argon2::{
        password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
        Argon2,
    };

    // Check if admin user already exists
    let admin_exists: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM users WHERE username = 'seok'"
    )
    .fetch_optional(pool)
    .await?;

    if let Some(_existing_id) = admin_exists {
        info!("Admin user already exists, skipping seed");
        return Ok(None);
    }

    info!("Creating admin user: seok");

    // Hash the password "long"
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(b"long", &salt)
        .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?
        .to_string();

    // Get Korean language ID
    let korean_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM languages WHERE code = 'ko'"
    )
    .fetch_optional(pool)
    .await?;

    // Create admin user with Korean as target language
    sqlx::query(
        "INSERT INTO users (username, password_hash, is_admin, target_language_id) VALUES (?, ?, 1, ?)"
    )
    .bind("seok")
    .bind(&password_hash)
    .bind(korean_id)
    .execute(pool)
    .await?;

    let user_id: i64 = sqlx::query_scalar(
        "SELECT id FROM users WHERE username = 'seok'"
    )
    .fetch_one(pool)
    .await?;

    info!("Admin user created successfully with Korean as target language");

    Ok(Some(user_id))
}

async fn seed_words_if_empty(pool: &SqlitePool) -> anyhow::Result<()> {
    // Check if words table is empty
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM words")
        .fetch_one(pool)
        .await?;

    if count > 0 {
        info!("Words table already has {} entries, skipping seed", count);
        return Ok(());
    }

    info!("Seeding words from decks/*.jsonl...");

    // Read all JSONL files from decks directory
    let decks_path = Path::new("./decks");
    if !decks_path.exists() {
        info!("No decks directory found, skipping word seeding");
        return Ok(());
    }

    let deck_files = fs::read_dir(decks_path)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext == "jsonl")
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    if deck_files.is_empty() {
        info!("No .jsonl files found in decks directory");
        return Ok(());
    }

    let mut total_words = 0;

    for deck_file in deck_files {
        let file_path = deck_file.path();
        let file_name = file_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        
        info!("Processing deck file: {}", file_name);

        // Determine language from filename (e.g., korean.jsonl -> ko)
        let language_code = if file_name.starts_with("korean") {
            "ko"
        } else {
            // Add more language mappings as needed
            info!("Unknown language for file {}, skipping", file_name);
            continue;
        };

        // Get language ID
        let language_id: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM languages WHERE code = ?"
        )
        .bind(language_code)
        .fetch_optional(pool)
        .await?;

        let Some(language_id) = language_id else {
            info!("Language {} not found in database, skipping {}", language_code, file_name);
            continue;
        };

        // Read and parse the JSONL file
        let contents = fs::read_to_string(&file_path)?;
        let lines: Vec<&str> = contents.lines().collect();

        // Skip first line (metadata) and process vocabulary entries
        for line in lines.iter().skip(1) {
            if line.trim().is_empty() {
                continue;
            }

            let entry: serde_json::Value = serde_json::from_str(line)?;
            
            // Extract word data only (skip review history fields like guess_count, wrong_guess_count, etc.)
            let homograph = &entry["homographs"][0];
            let sense = &homograph["senses"][0];
            let translation = &sense["translations"][0];
            let context = &sense["contexts"][0];
            
            let form = homograph["form"].as_str().unwrap_or("");
            
            // Skip if word already exists
            let existing: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM words WHERE form = ? AND language_id = ?"
            )
            .bind(form)
            .bind(language_id)
            .fetch_optional(pool)
            .await?;
            
            if existing.is_some() {
                continue;
            }
            
            let hint = translation["translation"].as_str().unwrap_or("");
            let context_text = context["context"].as_str().unwrap_or("");
            let context_translation = context["translations"][0]["translation"].as_str().unwrap_or("");
            let grammar = homograph["parsed_grammar"]["fragments"][0]["full"].as_str();
            
            // Extract politeness from comments
            let empty_vec = vec![];
            let comments = translation["comments"].as_array().unwrap_or(&empty_vec);
            let politeness_patterns = [
                "formal and casual speech",
                "formal and polite speech",
                "informal and casual speech",
                "informal and polite speech",
                "informal or formal situations",
            ];
            
            let mut politeness: Option<String> = None;
            let mut notes = Vec::new();
            
            for comment in comments {
                if let Some(comment_text) = comment["comment"].as_str() {
                    if politeness_patterns.iter().any(|p| comment_text.to_lowercase().contains(p)) {
                        politeness = Some(comment_text.to_string());
                    } else {
                        notes.push(comment_text.to_string());
                    }
                }
            }
            
            let notes_json = serde_json::to_string(&notes)?;
            
            // Insert word with language_id (no review history)
            sqlx::query(
                "INSERT INTO words (form, hint, context, context_translation, grammar, politeness, notes, language_id) 
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(form)
            .bind(hint)
            .bind(context_text)
            .bind(context_translation)
            .bind(grammar)
            .bind(politeness)
            .bind(&notes_json)
            .bind(language_id)
            .execute(pool)
            .await?;
            
            total_words += 1;
        }
    }

    info!("Successfully seeded {} words from all deck files", total_words);

    let final_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM words")
        .fetch_one(pool)
        .await?;

    info!("Total words in database: {}", final_count);

    Ok(())
}

