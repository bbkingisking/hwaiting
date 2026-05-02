use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::info;

use crate::error::AppError;
use crate::auth::AuthUser;

#[derive(Serialize)]
pub struct ExportData {
    pub exported_at: String,
    pub words: Vec<ExportWord>,
}

#[derive(Serialize)]
pub struct ExportWord {
    pub word_id: i64,
    pub form: String,
    pub hint: String,
    pub context: String,
    pub context_translation: String,
    pub grammar: Option<String>,
    pub politeness: Option<String>,
    pub notes: Vec<String>,
    pub card_state: Option<ExportCardState>,
    pub review_history: Vec<ExportReview>,
}

#[derive(Serialize)]
pub struct ExportCardState {
    pub stability: f64,
    pub difficulty: f64,
    pub last_review: String,
    pub due_date: String,
}

#[derive(Serialize)]
pub struct ExportReview {
    pub rating: i64,
    pub reviewed_at: String,
}

#[derive(Serialize)]
pub struct UserProfile {
    pub username: String,
    pub target_language: Option<LanguageInfo>,
}

#[derive(Serialize)]
pub struct LanguageInfo {
    pub id: i64,
    pub code: String,
    pub name: String,
}

#[derive(Deserialize)]
pub struct SetLanguageRequest {
    pub language_id: i64,
}

#[derive(Serialize)]
pub struct SetLanguageResponse {
    pub success: bool,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct ImportData {
    pub exported_at: String,
    pub words: Vec<ImportWord>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct ImportWord {
    pub word_id: i64,
    pub form: String,
    pub hint: String,
    pub context: String,
    pub context_translation: String,
    pub grammar: Option<String>,
    pub politeness: Option<String>,
    pub notes: Vec<String>,
    pub card_state: Option<ImportCardState>,
    pub review_history: Vec<ImportReview>,
}

#[derive(Deserialize)]
pub struct ImportCardState {
    pub stability: f64,
    pub difficulty: f64,
    pub last_review: String,
    pub due_date: String,
}

#[derive(Deserialize)]
pub struct ImportReview {
    pub rating: i64,
    pub reviewed_at: String,
}

#[derive(Serialize)]
pub struct ImportResponse {
    pub success: bool,
    pub words_imported: usize,
    pub reviews_imported: usize,
}

#[derive(Serialize)]
pub struct UserSettings {
    pub show_percentage: bool,
    pub red_threshold: i64,
    pub yellow_threshold: i64,
    pub day_boundary_hour: i64,
    pub auto_progress_on_correct: bool,
    pub auto_progress_delay: i64,
    pub suppress_new_cards: bool,
    pub desired_retention: f64,
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    pub show_percentage: Option<bool>,
    pub red_threshold: Option<i64>,
    pub yellow_threshold: Option<i64>,
    pub day_boundary_hour: Option<i64>,
    pub auto_progress_on_correct: Option<bool>,
    pub auto_progress_delay: Option<i64>,
    pub suppress_new_cards: Option<bool>,
    pub desired_retention: Option<f64>,
}

#[derive(Serialize)]
pub struct UpdateSettingsResponse {
    pub success: bool,
}

// Get current user's profile
pub async fn get_profile(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<UserProfile>, AppError> {
    let user_id = auth.0;
    info!("Getting profile for user_id: {}", user_id);

    let row = sqlx::query(
        r#"
        SELECT 
            u.username,
            l.id as lang_id,
            l.code as lang_code,
            l.name as lang_name
        FROM users u
        LEFT JOIN languages l ON l.id = u.target_language_id
        WHERE u.id = ?
        "#,
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    let username: String = row.get("username");
    let lang_id: Option<i64> = row.get("lang_id");
    
    let target_language = if let Some(id) = lang_id {
        Some(LanguageInfo {
            id,
            code: row.get("lang_code"),
            name: row.get("lang_name"),
        })
    } else {
        None
    };

    Ok(Json(UserProfile {
        username,
        target_language,
    }))
}

// Get list of available languages
pub async fn get_languages(
    State(pool): State<SqlitePool>,
) -> Result<Json<Vec<LanguageInfo>>, AppError> {
    info!("Getting list of available languages");

    let rows = sqlx::query("SELECT id, code, name FROM languages ORDER BY name")
        .fetch_all(&pool)
        .await?;

    let languages: Vec<LanguageInfo> = rows
        .iter()
        .map(|row| LanguageInfo {
            id: row.get("id"),
            code: row.get("code"),
            name: row.get("name"),
        })
        .collect();

    Ok(Json(languages))
}

// Set user's target language
pub async fn set_language(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Json(payload): Json<SetLanguageRequest>,
) -> Result<Json<SetLanguageResponse>, AppError> {
    let user_id = auth.0;
    info!("Setting language for user_id: {} to language_id: {}", user_id, payload.language_id);

    // Verify that the language exists
    let language_exists: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM languages WHERE id = ?"
    )
    .bind(payload.language_id)
    .fetch_optional(&pool)
    .await?;

    if language_exists.is_none() {
        return Err(AppError::Internal("Language not found".to_string()));
    }

    // Update user's target language
    sqlx::query("UPDATE users SET target_language_id = ? WHERE id = ?")
        .bind(payload.language_id)
        .bind(user_id)
        .execute(&pool)
        .await?;

    info!("Language set successfully");

    Ok(Json(SetLanguageResponse { success: true }))
}

// Export user's learning data
pub async fn export_data(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<ExportData>, AppError> {
    let user_id = auth.0;
    info!("Exporting data for user_id: {}", user_id);

    // Get user's target language
    let target_language_id: Option<i64> = sqlx::query_scalar(
        "SELECT target_language_id FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    let target_language_id = target_language_id
        .ok_or_else(|| AppError::Internal("User has no target language set".to_string()))?;

    // Get all words for the user's target language with their card states
    let word_rows = sqlx::query(
        r#"
        SELECT 
            w.id, w.form, w.hint, w.context, w.context_translation,
            w.grammar, w.politeness, w.notes,
            cs.stability, cs.difficulty, cs.last_review, cs.due_date
        FROM words w
        LEFT JOIN card_states cs ON cs.word_id = w.id AND cs.user_id = ?
        WHERE w.language_id = ?
        ORDER BY w.id
        "#,
    )
    .bind(user_id)
    .bind(target_language_id)
    .fetch_all(&pool)
    .await?;

    let mut words = Vec::new();

    for word_row in word_rows {
        let word_id: i64 = word_row.get("id");
        let form: String = word_row.get("form");
        let hint: String = word_row.get("hint");
        let context: String = word_row.get("context");
        let context_translation: String = word_row.get("context_translation");
        let grammar: Option<String> = word_row.get("grammar");
        let politeness: Option<String> = word_row.get("politeness");
        let notes_json: String = word_row.get("notes");
        let notes: Vec<String> = serde_json::from_str(&notes_json).unwrap_or_default();

        // Get card state if exists
        let card_state = if let (Some(stability), Some(difficulty), Some(last_review), Some(due_date)) = (
            word_row.get::<Option<f64>, _>("stability"),
            word_row.get::<Option<f64>, _>("difficulty"),
            word_row.get::<Option<String>, _>("last_review"),
            word_row.get::<Option<String>, _>("due_date"),
        ) {
            Some(ExportCardState {
                stability,
                difficulty,
                last_review,
                due_date,
            })
        } else {
            None
        };

        // Get review history for this word
        let review_rows = sqlx::query(
            "SELECT rating, reviewed_at FROM review_history WHERE user_id = ? AND word_id = ? ORDER BY reviewed_at"
        )
        .bind(user_id)
        .bind(word_id)
        .fetch_all(&pool)
        .await?;

        let review_history: Vec<ExportReview> = review_rows
            .iter()
            .map(|row| ExportReview {
                rating: row.get("rating"),
                reviewed_at: row.get("reviewed_at"),
            })
            .collect();

        words.push(ExportWord {
            word_id,
            form,
            hint,
            context,
            context_translation,
            grammar,
            politeness,
            notes,
            card_state,
            review_history,
        });
    }

    let exported_at = chrono::Utc::now().to_rfc3339();

    Ok(Json(ExportData {
        exported_at,
        words,
    }))
}

// Import user's learning data
pub async fn import_data(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Json(payload): Json<ImportData>,
) -> Result<Json<ImportResponse>, AppError> {
    let user_id = auth.0;
    info!("Importing data for user_id: {}", user_id);

    // Get user's target language
    let target_language_id: Option<i64> = sqlx::query_scalar(
        "SELECT target_language_id FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    let target_language_id = target_language_id
        .ok_or_else(|| AppError::Internal("User has no target language set".to_string()))?;

    // Start a transaction to ensure atomicity
    let mut tx = pool.begin().await?;

    let mut words_imported = 0;
    let mut reviews_imported = 0;

    for import_word in payload.words {
        // Find the word in the database by matching content
        // We can't rely on word_id as it might be different in different databases
        let word_row = sqlx::query(
            r#"
            SELECT id FROM words 
            WHERE form = ? AND hint = ? AND context = ? AND language_id = ?
            LIMIT 1
            "#
        )
        .bind(&import_word.form)
        .bind(&import_word.hint)
        .bind(&import_word.context)
        .bind(target_language_id)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(word_row) = word_row else {
            info!("Word not found in database, skipping: {}", import_word.form);
            continue;
        };

        let word_id: i64 = word_row.get("id");

        // Import card state if exists
        if let Some(card_state) = import_word.card_state {
            sqlx::query(
                r#"
                INSERT INTO card_states (user_id, word_id, stability, difficulty, last_review, due_date)
                VALUES (?, ?, ?, ?, ?, ?)
                ON CONFLICT(user_id, word_id) DO UPDATE SET
                    stability = excluded.stability,
                    difficulty = excluded.difficulty,
                    last_review = excluded.last_review,
                    due_date = excluded.due_date
                "#
            )
            .bind(user_id)
            .bind(word_id)
            .bind(card_state.stability)
            .bind(card_state.difficulty)
            .bind(&card_state.last_review)
            .bind(&card_state.due_date)
            .execute(&mut *tx)
            .await?;

            words_imported += 1;
        }

        // Import review history
        for review in import_word.review_history {
            // Check if this exact review already exists (to avoid duplicates)
            let exists: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM review_history WHERE user_id = ? AND word_id = ? AND rating = ? AND reviewed_at = ?"
            )
            .bind(user_id)
            .bind(word_id)
            .bind(review.rating)
            .bind(&review.reviewed_at)
            .fetch_optional(&mut *tx)
            .await?;

            if exists.is_none() {
                sqlx::query(
                    "INSERT INTO review_history (user_id, word_id, rating, reviewed_at) VALUES (?, ?, ?, ?)"
                )
                .bind(user_id)
                .bind(word_id)
                .bind(review.rating)
                .bind(&review.reviewed_at)
                .execute(&mut *tx)
                .await?;

                reviews_imported += 1;
            }
        }
    }

    // Commit the transaction
    tx.commit().await?;

    info!(
        "Import complete: {} words, {} reviews",
        words_imported, reviews_imported
    );

    Ok(Json(ImportResponse {
        success: true,
        words_imported,
        reviews_imported,
    }))
}

// Get user settings
pub async fn get_settings(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<UserSettings>, AppError> {
    let user_id = auth.0;
    info!("Getting settings for user_id: {}", user_id);

    // Ensure user_settings row exists
    sqlx::query(
        r#"
        INSERT INTO user_settings (user_id)
        VALUES (?)
        ON CONFLICT(user_id) DO NOTHING
        "#
    )
    .bind(user_id)
    .execute(&pool)
    .await?;

    let row = sqlx::query(
        r#"
        SELECT show_percentage, red_threshold, yellow_threshold, day_boundary_hour, auto_progress_on_correct, auto_progress_delay, suppress_new_cards, desired_retention
        FROM user_settings
        WHERE user_id = ?
        "#
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    Ok(Json(UserSettings {
        show_percentage: row.get("show_percentage"),
        red_threshold: row.get("red_threshold"),
        yellow_threshold: row.get("yellow_threshold"),
        day_boundary_hour: row.get("day_boundary_hour"),
        auto_progress_on_correct: row.get("auto_progress_on_correct"),
        auto_progress_delay: row.get("auto_progress_delay"),
        suppress_new_cards: row.get("suppress_new_cards"),
        desired_retention: row.get("desired_retention"),
    }))
}

// Update user settings
pub async fn update_settings(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    Json(payload): Json<UpdateSettingsRequest>,
) -> Result<Json<UpdateSettingsResponse>, AppError> {
    let user_id = auth.0;
    info!("Updating settings for user_id: {}", user_id);

    // Ensure user_settings row exists
    sqlx::query(
        r#"
        INSERT INTO user_settings (user_id)
        VALUES (?)
        ON CONFLICT(user_id) DO NOTHING
        "#
    )
    .bind(user_id)
    .execute(&pool)
    .await?;

    // Update individual fields if provided
    if let Some(show_percentage) = payload.show_percentage {
        sqlx::query("UPDATE user_settings SET show_percentage = ? WHERE user_id = ?")
            .bind(show_percentage)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(red_threshold) = payload.red_threshold {
        sqlx::query("UPDATE user_settings SET red_threshold = ? WHERE user_id = ?")
            .bind(red_threshold)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(yellow_threshold) = payload.yellow_threshold {
        sqlx::query("UPDATE user_settings SET yellow_threshold = ? WHERE user_id = ?")
            .bind(yellow_threshold)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(day_boundary_hour) = payload.day_boundary_hour {
        // Validate hour is between 0 and 23
        if day_boundary_hour < 0 || day_boundary_hour > 23 {
            return Err(AppError::Internal("day_boundary_hour must be between 0 and 23".to_string()));
        }
        sqlx::query("UPDATE user_settings SET day_boundary_hour = ? WHERE user_id = ?")
            .bind(day_boundary_hour)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(auto_progress_on_correct) = payload.auto_progress_on_correct {
        sqlx::query("UPDATE user_settings SET auto_progress_on_correct = ? WHERE user_id = ?")
            .bind(auto_progress_on_correct)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(auto_progress_delay) = payload.auto_progress_delay {
        if auto_progress_delay < 0 || auto_progress_delay > 3000 {
            return Err(AppError::Internal("auto_progress_delay must be between 0 and 3000".to_string()));
        }
        sqlx::query("UPDATE user_settings SET auto_progress_delay = ? WHERE user_id = ?")
            .bind(auto_progress_delay)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(suppress_new_cards) = payload.suppress_new_cards {
        sqlx::query("UPDATE user_settings SET suppress_new_cards = ? WHERE user_id = ?")
            .bind(suppress_new_cards)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    if let Some(desired_retention) = payload.desired_retention {
        if desired_retention < 0.5 || desired_retention > 0.99 {
            return Err(AppError::Internal("desired_retention must be between 0.5 and 0.99".to_string()));
        }
        sqlx::query("UPDATE user_settings SET desired_retention = ? WHERE user_id = ?")
            .bind(desired_retention)
            .bind(user_id)
            .execute(&pool)
            .await?;
    }

    info!("Settings updated successfully");

    Ok(Json(UpdateSettingsResponse { success: true }))
}
