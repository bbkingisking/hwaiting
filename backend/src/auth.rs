use axum::{
    extract::{State, FromRequestParts},
    http::request::Parts,
    Json,
};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use serde::{Deserialize, Serialize};
use sqlx::{SqlitePool, Row};
use tracing::{debug, info, warn};

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

    info!("Login attempt for user: {}", username);

    // Check if user exists
    let user = sqlx::query("SELECT id, username, password_hash FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await?;

    debug!("User lookup result: {}", if user.is_some() { "found" } else { "not found" });

    match user {
        Some(row) => {
            debug!("Existing user found, verifying password");
            
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
                info!("Password verified successfully for user: {}", username);
                // Generate JWT token
                let token = generate_token(user_id)?;
                Ok(Json(AuthResponse {
                    token,
                    username: stored_username,
                }))
            } else {
                warn!("Invalid password attempt for user: {}", username);
                Err(AppError::InvalidCredentials)
            }
        }
        None => {
            info!("Creating new user: {}", username);
            
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
            info!("User created successfully with id: {}", user_id);
            
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
    use jsonwebtoken::{encode, EncodingKey, Header};
    
    let claims = Claims { sub: user_id };
    
    let mut header = Header::default();
    header.alg = Algorithm::HS256;
    
    encode(
        &header,
        &claims,
        &EncodingKey::from_secret(SECRET.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("Failed to generate token: {}", e)))
}

// JWT Claims
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64, // user_id
}

// Auth extractor - extracts user_id from JWT token
pub struct AuthUser(pub i64);

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Extract Authorization header
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .ok_or(AppError::InvalidCredentials)?;

        // Remove "Bearer " prefix
        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AppError::InvalidCredentials)?;

        // Decode and validate token
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(SECRET.as_bytes()),
            &Validation::new(Algorithm::HS256),
        )
        .map_err(|_| AppError::InvalidCredentials)?;

        Ok(AuthUser(token_data.claims.sub))
    }
}

const SECRET: &str = "your-secret-key-change-this-in-production";