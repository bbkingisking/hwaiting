use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;
use tracing::info;

pub async fn init() -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str("sqlite:annyeong.db")?
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

async fn seed_admin_user(pool: &SqlitePool) -> anyhow::Result<()> {
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

    if admin_exists.is_some() {
        info!("Admin user already exists, skipping seed");
        return Ok(());
    }

    info!("Creating admin user: seok");

    // Hash the password "long"
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(b"long", &salt)
        .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?
        .to_string();

    // Create admin user
    sqlx::query(
        "INSERT INTO users (username, password_hash, is_admin) VALUES (?, ?, 1)"
    )
    .bind("seok")
    .bind(&password_hash)
    .execute(pool)
    .await?;

    info!("Admin user created successfully");

    Ok(())
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

    info!("Seeding words from words.json...");

    // Read and parse words.json
    let words_json = include_str!("../../words.json");
    let lines: Vec<&str> = words_json.lines().collect();

    // Skip first line (metadata)
    for (_idx, line) in lines.iter().skip(1).enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let entry: serde_json::Value = serde_json::from_str(line)?;
        
        // Extract word data
        let homograph = &entry["homographs"][0];
        let sense = &homograph["senses"][0];
        let translation = &sense["translations"][0];
        let context = &sense["contexts"][0];
        
        let form = homograph["form"].as_str().unwrap_or("");
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
        
        // Insert word
        sqlx::query(
            "INSERT INTO words (form, hint, context, context_translation, grammar, politeness, notes) 
             VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(form)
        .bind(hint)
        .bind(context_text)
        .bind(context_translation)
        .bind(grammar)
        .bind(politeness)
        .bind(&notes_json)
        .execute(pool)
        .await?;
    }

    let final_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM words")
        .fetch_one(pool)
        .await?;

    info!("Successfully seeded {} words", final_count);

    Ok(())
}