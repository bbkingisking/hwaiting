use axum::{
    extract::{FromRequest, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::de::DeserializeOwned;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Password hashing error")]
    PasswordHash,

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Invalid invite code")]
    InvalidInviteCode,

    #[error("Username already exists")]
    UsernameExists,

    #[error("Forbidden")]
    Forbidden,

    #[error("Not found")]
    NotFound,

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<argon2::password_hash::Error> for AppError {
    fn from(_: argon2::password_hash::Error) -> Self {
        AppError::PasswordHash
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::Database(ref e) => {
                eprintln!("Database error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string())
            }
            AppError::PasswordHash => {
                eprintln!("Password hash error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Authentication error".to_string())
            }
            AppError::InvalidCredentials => {
                (StatusCode::UNAUTHORIZED, "Invalid credentials".to_string())
            }
            AppError::InvalidInviteCode => {
                (StatusCode::BAD_REQUEST, "Invalid or already used invite code".to_string())
            }
            AppError::UsernameExists => {
                (StatusCode::CONFLICT, "Username already exists".to_string())
            }
            AppError::Forbidden => {
                (StatusCode::FORBIDDEN, "Forbidden".to_string())
            }
            AppError::NotFound => {
                (StatusCode::NOT_FOUND, "Not found".to_string())
            }
            AppError::BadRequest(ref msg) => {
                (StatusCode::BAD_REQUEST, msg.clone())
            }
            AppError::Internal(ref msg) => {
                eprintln!("Internal error: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
            }
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

/// Drop-in replacement for `axum::Json` as a request extractor. Identical on
/// success; on failure (malformed JSON, missing/mistyped fields) it converts
/// axum's default `JsonRejection` into `AppError::BadRequest`, so every error
/// this API returns - including body-parsing failures - shares the same
/// `{"error": "..."}` envelope and status-code convention, instead of a
/// plain-text 422 that bypasses `AppError` entirely.
pub struct AppJson<T>(pub T);

impl<S, T> FromRequest<S> for AppJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(AppJson(value)),
            Err(rejection) => Err(AppError::BadRequest(rejection.body_text())),
        }
    }
}
