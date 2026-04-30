use axum::{
    extract::State,
    Json,
};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use serde::{Deserialize, Serialize};
use sqlx::{SqlitePool, Row};

use crate::error::AppError;

#[derive(Deserialize)]
pub struct AuthRequest {
    pub who: String,
    pub really: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub username: String,
}

pub async fn login(
    State(pool): State<SqlitePool>,
    Json(payload): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let username = payload.who.trim();
    let password = payload.really.trim();

    // Check if user exists
    let user = sqlx::query("SELECT id, username, password_hash FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await?;

    match user {
        Some(row) => {
            // User exists - verify password
            let user_id: i64 = row.get("id");
            let stored_username: String = row.get("username");
            let password_hash: String = row.get("password_hash");
            
            // Parse the stored hash
            let parsed_hash = PasswordHash::new(&password_hash)?;
            
            // Verify password
            let password_matches = Argon2::default()
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok();
            
            if password_matches {
                // Generate JWT token
                let token = generate_token(user_id)?;
                Ok(Json(AuthResponse {
                    token,
                    username: stored_username,
                }))
            } else {
                Err(AppError::InvalidCredentials)
            }
        }
        None => {
            // User doesn't exist - create new user
            let salt = SaltString::generate(&mut OsRng);
            let argon2 = Argon2::default();
            let password_hash = argon2
                .hash_password(password.as_bytes(), &salt)?
                .to_string();
            
            let result = sqlx::query("INSERT INTO users (username, password_hash) VALUES (?, ?)")
                .bind(username)
                .bind(&password_hash)
                .execute(&pool)
                .await?;

            let user_id = result.last_insert_rowid();
            
            // Generate JWT token
            let token = generate_token(user_id)?;
            
            Ok(Json(AuthResponse {
                token,
                username: username.to_string(),
            }))
        }
    }
}

fn generate_token(user_id: i64) -> Result<String, AppError> {
    use jsonwebtoken::{encode, EncodingKey, Header, Algorithm};
    
    #[derive(Serialize)]
    struct Claims {
        sub: i64,
    }
    
    let claims = Claims { sub: user_id };
    
    // In production, this should be an environment variable
    let secret = "your-secret-key-change-this-in-production";
    
    let mut header = Header::default();
    header.alg = Algorithm::HS256;
    
    encode(
        &header,
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("Failed to generate token: {}", e)))
}