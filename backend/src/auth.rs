use axum::{
    extract::{State, FromRef, FromRequestParts},
    http::{request::Parts, StatusCode},
    Json,
};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use serde::{Deserialize, Serialize};
use sqlx::{SqlitePool, Row};
use tracing::{debug, info, warn};
use utoipa::ToSchema;

use crate::error::{AppError, AppJson};

#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize, ToSchema)]
pub struct SignupRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, ToSchema)]
pub struct AuthResponse {
    pub token: String,
    /// `None` for an account created via passkey, which has no username -
    /// see [`crate::passkey`].
    pub username: Option<String>,
    pub is_admin: bool,
}

#[utoipa::path(
    post,
    path = "/api/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = AuthResponse),
        (status = 400, description = "Malformed request body", body = crate::error::ErrorResponse),
        (status = 401, description = "Invalid credentials", body = crate::error::ErrorResponse),
    ),
    tag = "auth"
)]
pub async fn login(
    State(pool): State<SqlitePool>,
    AppJson(payload): AppJson<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let username = payload.username.trim();
    let password = payload.password.trim();

    info!("Login attempt for user: {}", username);

    // Check if user exists
    let user = sqlx::query("SELECT id, username, password_hash, is_admin FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await?;

    debug!("User lookup result: {}", if user.is_some() { "found" } else { "not found" });

    match user {
        Some(row) => {
            debug!("User found, verifying password");

            let user_id: i64 = row.get("id");
            let stored_username: String = row.get("username");
            let password_hash: Option<String> = row.get("password_hash");
            let is_admin: bool = row.get("is_admin");

            // A passkey-only account has no password_hash to check against -
            // password login for it is correctly impossible, not just unset.
            let Some(password_hash) = password_hash else {
                warn!("Password login attempt for passkey-only account: {}", username);
                return Err(AppError::InvalidCredentials);
            };

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
                    username: Some(stored_username),
                    is_admin,
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

#[utoipa::path(
    post,
    path = "/api/auth/signup",
    request_body = SignupRequest,
    responses(
        (status = 201, description = "Account created", body = AuthResponse),
        (status = 400, description = "Malformed request body", body = crate::error::ErrorResponse),
        (status = 409, description = "Username already exists", body = crate::error::ErrorResponse),
    ),
    tag = "auth"
)]
pub async fn signup(
    State(pool): State<SqlitePool>,
    AppJson(payload): AppJson<SignupRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), AppError> {
    let username = payload.username.trim();
    let password = payload.password.trim();

    info!("Signup attempt for user: {}", username);

    // One transaction for the whole thing: checking for an existing
    // username and creating the user need to see (and commit) a consistent
    // view, or a concurrent signup could slip in between the check and the
    // insert and both succeed for the same username.
    let mut tx = pool.begin().await?;

    // Check if username already exists
    let existing_user = sqlx::query("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&mut *tx)
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
        .execute(&mut *tx)
        .await?;

    let user_id = result.last_insert_rowid();
    info!("User created successfully with id: {}", user_id);

    // New users are not admins by default
    let is_admin = false;

    tx.commit().await?;

    // Generate JWT token
    let token = generate_token(user_id)?;

    Ok((StatusCode::CREATED, Json(AuthResponse {
        token,
        username: Some(username.to_string()),
        is_admin,
    })))
}

/// The parsing behind `jwt_ttl_seconds`, taking the raw config value
/// directly rather than reading it from `credentials` itself - split out so
/// the parsing/validation rules can be tested without touching process
/// environment.
fn parse_jwt_ttl_seconds(raw: Option<String>) -> Option<i64> {
    let raw = raw?;
    let secs: i64 = raw
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("HWAITING_JWT_EXPIRY_SECONDS must be a non-negative integer, got '{}'", raw));

    if secs < 0 {
        panic!("HWAITING_JWT_EXPIRY_SECONDS must be a non-negative integer, got '{}'", raw);
    }

    (secs > 0).then_some(secs)
}

/// TTL for newly issued JWTs, in seconds, from `HWAITING_JWT_EXPIRY_SECONDS`. Unset or
/// `0` means tokens never expire - the default, so existing single-user
/// deployments are unaffected unless this is set explicitly. A negative
/// value is a config error and panics at token-generation time, same as an
/// unparseable one.
fn jwt_ttl_seconds() -> Option<i64> {
    parse_jwt_ttl_seconds(crate::credentials::jwt_expiry_seconds())
}

/// The actual encode step behind `generate_token`, taking the secret and TTL
/// as plain arguments rather than reading them from `credentials`/
/// `jwt_ttl_seconds` itself - split out so token generation can be tested
/// without touching process environment or credential files.
fn encode_token(user_id: i64, secret: &str, ttl_seconds: Option<i64>) -> Result<String, AppError> {
    use jsonwebtoken::{encode, EncodingKey, Header};

    let exp = ttl_seconds.map(|ttl| (Utc::now() + Duration::seconds(ttl)).timestamp());

    let claims = Claims { sub: user_id, exp };

    let mut header = Header::default();
    header.alg = Algorithm::HS256;

    encode(&header, &claims, &EncodingKey::from_secret(secret.as_bytes()))
        .map_err(|e| AppError::Internal(format!("Failed to generate token: {}", e)))
}

/// The actual decode+validate step behind `AuthUser`'s extractor, taking the
/// secret as a plain argument for the same testability reason as
/// `encode_token`. `exp`, when present, is still validated (rejecting an
/// expired token) even though it's never required to be present.
fn decode_token(token: &str, secret: &str) -> Result<Claims, AppError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.required_spec_claims.clear(); // Don't require exp, iat, etc.

    decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &validation)
        .map(|data| data.claims)
        .map_err(|e| {
            warn!("Token validation failed: {:?}", e);
            AppError::InvalidCredentials
        })
}

/// Crate-visible (not just this module's) since [`crate::passkey`]'s
/// login/register-finish handlers issue the exact same JWT this
/// username/password path does - passkey auth is just another way to reach
/// this function, not a separate token scheme.
pub(crate) fn generate_token(user_id: i64) -> Result<String, AppError> {
    encode_token(user_id, &crate::credentials::jwt_secret(), jwt_ttl_seconds())
}

// JWT Claims
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64, // user_id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>, // unix timestamp; absent means the token never expires
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

        let claims = decode_token(token, &crate::credentials::jwt_secret())?;

        debug!("Token validated successfully for user_id: {}", claims.sub);
        Ok(AuthUser(claims.sub))
    }
}

// Admin auth extractor - extracts user_id from JWT token and verifies admin status
#[allow(dead_code)]
pub struct AdminUser(pub i64);

impl<S> FromRequestParts<S> for AdminUser
where
    S: Send + Sync,
    SqlitePool: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // First, extract the user using AuthUser
        let AuthUser(user_id) = AuthUser::from_request_parts(parts, state).await?;
        let pool = SqlitePool::from_ref(state);

        // Check if user is admin
        let is_admin: bool = sqlx::query_scalar(
            "SELECT is_admin FROM users WHERE id = ?"
        )
        .bind(user_id)
        .fetch_optional(&pool)
        .await?
        .unwrap_or(false);

        if !is_admin {
            warn!("Non-admin user {} attempted to access admin endpoint", user_id);
            return Err(AppError::Forbidden);
        }

        info!("Admin user {} authenticated", user_id);
        Ok(AdminUser(user_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_jwt_ttl_seconds -----------------------------------------------

    #[test]
    fn ttl_unset_is_none() {
        assert_eq!(parse_jwt_ttl_seconds(None), None);
    }

    #[test]
    fn ttl_zero_is_none() {
        assert_eq!(parse_jwt_ttl_seconds(Some("0".to_string())), None);
    }

    #[test]
    fn ttl_positive_is_some() {
        assert_eq!(parse_jwt_ttl_seconds(Some("3600".to_string())), Some(3600));
    }

    #[test]
    fn ttl_tolerates_surrounding_whitespace() {
        assert_eq!(parse_jwt_ttl_seconds(Some("  60  ".to_string())), Some(60));
    }

    #[test]
    #[should_panic(expected = "must be a non-negative integer")]
    fn ttl_negative_panics() {
        parse_jwt_ttl_seconds(Some("-1".to_string()));
    }

    #[test]
    #[should_panic(expected = "must be a non-negative integer")]
    fn ttl_non_numeric_panics() {
        parse_jwt_ttl_seconds(Some("soon".to_string()));
    }

    // --- encode_token / decode_token -----------------------------------------

    #[test]
    fn round_trips_a_token() {
        let token = encode_token(42, "test-secret", None).unwrap();
        let claims = decode_token(&token, "test-secret").unwrap();
        assert_eq!(claims.sub, 42);
        assert_eq!(claims.exp, None);
    }

    // No separate "token without exp is accepted" / "future exp is
    // accepted" cases: both are the same round_trips_a_token scenario
    // (encode then decode succeeds) with different data, and neither adds
    // coverage beyond what expired_token_is_rejected already implies (if
    // exp weren't wired up at all, that test would fail).

    #[test]
    fn expired_token_is_rejected() {
        // Build a token with exp already in the past directly, rather than
        // via encode_token (which always computes exp relative to now).
        use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
        // Well beyond jsonwebtoken's default 60s leeway, so this isn't a
        // borderline case.
        let claims = Claims {
            sub: 1,
            exp: Some((Utc::now() - Duration::seconds(3600)).timestamp()),
        };
        let mut header = Header::default();
        header.alg = Algorithm::HS256;
        let token = encode(&header, &claims, &EncodingKey::from_secret(b"s")).unwrap();

        assert!(matches!(decode_token(&token, "s"), Err(AppError::InvalidCredentials)));
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let token = encode_token(1, "right-secret", None).unwrap();
        assert!(matches!(
            decode_token(&token, "wrong-secret"),
            Err(AppError::InvalidCredentials)
        ));
    }

    // No "garbage token is rejected" test: that exercises jsonwebtoken's
    // own parse-failure path, not any logic of ours.
}
