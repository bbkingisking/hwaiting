use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;
use tracing::info;
use chrono::{DateTime, Utc};

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
    let admin_user_id = seed_admin_user(&pool).await?;

    // Seed words if table is empty
    seed_words_if_empty(&pool).await?;

    // Seed admin learning history if admin user was just created
    if let Some(user_id) = admin_user_id {
        seed_admin_learning_history(&pool, user_id).await?;
    }

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

    info!("Seeding words from words.json...");

    // Get Korean language ID
    let korean_id: i64 = sqlx::query_scalar(
        "SELECT id FROM languages WHERE code = 'ko'"
    )
    .fetch_one(pool)
    .await?;

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
        
        // Insert word with language_id
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
        .bind(korean_id)
        .execute(pool)
        .await?;
    }

    let final_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM words")
        .fetch_one(pool)
        .await?;

    info!("Successfully seeded {} words", final_count);

    Ok(())
}

async fn seed_admin_learning_history(pool: &SqlitePool, user_id: i64) -> anyhow::Result<()> {
    info!("Seeding admin learning history from words.json...");

    // Read and parse words.json
    let words_json = include_str!("../../words.json");
    let lines: Vec<&str> = words_json.lines().collect();

    let mut history_count = 0;
    let mut card_states_count = 0;

    // Skip first line (metadata)
    for line in lines.iter().skip(1) {
        if line.trim().is_empty() {
            continue;
        }

        let entry: serde_json::Value = serde_json::from_str(line)?;
        
        // Extract word form to find matching word_id
        let homograph = &entry["homographs"][0];
        let form = homograph["form"].as_str().unwrap_or("");
        
        // Find the word_id for this form
        let word_id: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM words WHERE form = ?"
        )
        .bind(form)
        .fetch_optional(pool)
        .await?;

        let Some(word_id) = word_id else {
            continue;
        };

        // Extract learning history data
        let guess_count = entry["guess_count"].as_i64().unwrap_or(0);
        let wrong_guess_count = entry["wrong_guess_count"].as_i64().unwrap_or(0);
        let last_correct = entry["last_correct"].as_bool().unwrap_or(false);
        let last_guess_ts = entry["last_guess_ts"].as_str();

        if guess_count == 0 {
            continue;
        }

        // Parse the timestamp
        let reviewed_at = if let Some(ts) = last_guess_ts {
            DateTime::parse_from_rfc3339(ts)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        } else {
            None
        };

        // Create review history entries based on guess_count
        // We'll simulate the review history by creating entries
        let correct_count = guess_count - wrong_guess_count;
        
        // Create entries for wrong guesses (rating 1 = Again)
        for i in 0..wrong_guess_count {
            let offset_days = (guess_count - i - 1) * 2; // Space them out
            let review_time = reviewed_at
                .map(|dt| dt - chrono::Duration::days(offset_days))
                .unwrap_or_else(Utc::now);

            sqlx::query(
                "INSERT INTO review_history (user_id, word_id, rating, reviewed_at) VALUES (?, ?, 1, ?)"
            )
            .bind(user_id)
            .bind(word_id)
            .bind(review_time)
            .execute(pool)
            .await?;
            
            history_count += 1;
        }

        // Create entries for correct guesses (rating 3 = Good)
        for i in 0..correct_count {
            let offset_days = (correct_count - i - 1) * 2;
            let review_time = reviewed_at
                .map(|dt| dt - chrono::Duration::days(offset_days))
                .unwrap_or_else(Utc::now);

            sqlx::query(
                "INSERT INTO review_history (user_id, word_id, rating, reviewed_at) VALUES (?, ?, 3, ?)"
            )
            .bind(user_id)
            .bind(word_id)
            .bind(review_time)
            .execute(pool)
            .await?;
            
            history_count += 1;
        }

        // Create card_state based on performance
        // Use heuristics to estimate FSRS parameters from the old data
        let correct_rate = correct_count as f64 / guess_count as f64;
        
        // Estimate stability based on correct rate and guess count
        // Higher correct rate and more reviews = higher stability
        let stability = if correct_rate > 0.7 {
            (guess_count as f64 * 0.5).min(30.0).max(1.0)
        } else if correct_rate > 0.4 {
            (guess_count as f64 * 0.3).min(10.0).max(0.5)
        } else {
            (guess_count as f64 * 0.1).min(3.0).max(0.3)
        };

        // Estimate difficulty: lower correct rate = higher difficulty
        let difficulty = (10.0 - (correct_rate * 10.0)).max(1.0).min(10.0);

        let last_review = reviewed_at.unwrap_or_else(Utc::now);
        
        // Calculate due date based on stability
        // If last was correct, add stability days; if wrong, due now
        let due_date = if last_correct {
            last_review + chrono::Duration::days(stability as i64)
        } else {
            Utc::now()
        };

        sqlx::query(
            "INSERT INTO card_states (user_id, word_id, stability, difficulty, last_review, due_date) 
             VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(user_id)
        .bind(word_id)
        .bind(stability)
        .bind(difficulty)
        .bind(last_review)
        .bind(due_date)
        .execute(pool)
        .await?;

        card_states_count += 1;
    }

    info!("Successfully seeded {} review history entries and {} card states for admin user", 
          history_count, card_states_count);

    Ok(())
}