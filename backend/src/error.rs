use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
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
    
    #[error("Unauthorized")]
    Unauthorized,
    
    #[error("Not found")]
    NotFound,
    
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
                (StatusCode::INTERNAL_SERVER_ERROR, "Database error")
            }
            AppError::PasswordHash => {
                eprintln!("Password hash error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Authentication error")
            }
            AppError::InvalidCredentials => {
                (StatusCode::UNAUTHORIZED, "Invalid credentials")
            }
            AppError::InvalidInviteCode => {
                (StatusCode::BAD_REQUEST, "Invalid or already used invite code")
            }
            AppError::UsernameExists => {
                (StatusCode::CONFLICT, "Username already exists")
            }
            AppError::Unauthorized => {
                (StatusCode::FORBIDDEN, "Unauthorized")
            }
            AppError::NotFound => {
                (StatusCode::NOT_FOUND, "Not found")
            }
            AppError::Internal(ref msg) => {
                eprintln!("Internal error: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}