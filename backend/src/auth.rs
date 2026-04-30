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
pub struct LoginRequest {
    pub who: String,
    pub really: String,
}

#[derive(Deserialize)]
pub struct SignupRequest {
    pub who: String,
    pub really: String,
    pub invite_code: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub username: String,
}

pub async fn login(
    State(pool): State<SqlitePool>,
    Json(payload): Json<LoginRequest>,
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
            debug!("User found, verifying password");
            
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
            warn!("Login attempt for non-existent user: {}", username);
            Err(AppError::InvalidCredentials)
        }
    }
}

pub async fn signup(
    State(pool): State<SqlitePool>,
    Json(payload): Json<SignupRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let username = payload.who.trim();
    let password = payload.really.trim();
    let invite_code = payload.invite_code.trim();

    info!("Signup attempt for user: {}", username);

    // Validate invite code
    let invite = sqlx::query(
        "SELECT id, used_at FROM invite_codes WHERE code = ?"
    )
    .bind(invite_code)
    .fetch_optional(&pool)
    .await?;

    match invite {
        Some(row) => {
            let used_at: Option<String> = row.get("used_at");
            if used_at.is_some() {
                warn!("Signup attempt with already used invite code");
                return Err(AppError::InvalidInviteCode);
            }
        }
        None => {
            warn!("Signup attempt with invalid invite code");
            return Err(AppError::InvalidInviteCode);
        }
    }

    // Check if username already exists
    let existing_user = sqlx::query("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await?;

    if existing_user.is_some() {
        warn!("Signup attempt with existing username: {}", username);
        return Err(AppError::UsernameExists);
    }

    // Create new user
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

    // Mark invite code as used
    sqlx::query("UPDATE invite_codes SET used_at = CURRENT_TIMESTAMP, used_by_user_id = ? WHERE code = ?")
        .bind(user_id)
        .bind(invite_code)
        .execute(&pool)
        .await?;

    info!("Invite code marked as used");
    
    // Generate JWT token
    let token = generate_token(user_id)?;
    
    Ok(Json(AuthResponse {
        token,
        username: username.to_string(),
    }))
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
        debug!("AuthUser extractor called");
        
        // Extract Authorization header
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .ok_or_else(|| {
                warn!("Missing Authorization header");
                AppError::InvalidCredentials
            })?;

        debug!("Authorization header present");

        // Remove "Bearer " prefix
        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| {
                warn!("Invalid Authorization header format (missing Bearer prefix)");
                AppError::InvalidCredentials
            })?;

        debug!("Token extracted, attempting to decode");

        // Decode and validate token (no expiration check since tokens never expire)
        let mut validation = Validation::new(Algorithm::HS256);
        validation.required_spec_claims.clear(); // Don't require exp, iat, etc.
        
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(SECRET.as_bytes()),
            &validation,
        )
        .map_err(|e| {
            warn!("Token validation failed: {:?}", e);
            AppError::InvalidCredentials
        })?;

        info!("Token validated successfully for user_id: {}", token_data.claims.sub);
        Ok(AuthUser(token_data.claims.sub))
    }
}

// Admin auth extractor - extracts user_id from JWT token and verifies admin status
pub struct AdminUser(pub i64);

impl FromRequestParts<SqlitePool> for AdminUser
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &SqlitePool) -> Result<Self, Self::Rejection> {
        // First, extract the user using AuthUser
        let AuthUser(user_id) = AuthUser::from_request_parts(parts, state).await?;
        
        // Check if user is admin
        let is_admin: bool = sqlx::query_scalar(
            "SELECT is_admin FROM users WHERE id = ?"
        )
        .bind(user_id)
        .fetch_optional(state)
        .await?
        .unwrap_or(false);
        
        if !is_admin {
            warn!("Non-admin user {} attempted to access admin endpoint", user_id);
            return Err(AppError::Unauthorized);
        }
        
        info!("Admin user {} authenticated", user_id);
        Ok(AdminUser(user_id))
    }
}

const SECRET: &str = "your-secret-key-change-this-in-production";