use axum::{
    extract::{FromRequest, FromRequestParts, OptionalFromRequest, Path, Query, Request},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;
use utoipa::ToSchema;

/// Schema-only mirror of the `{"error": "..."}` envelope every error
/// response uses (built ad hoc via `serde_json::json!` in
/// `AppError::into_response`, not actually constructed from this struct) -
/// exists purely so `#[utoipa::path]` annotations have something to point
/// error responses at.
#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Password hashing error")]
    PasswordHash,

    #[error("Invalid credentials")]
    InvalidCredentials,

    /// A `navigator.credentials.create()`/`.get()` ceremony failed
    /// server-side verification (bad signature, origin/RP ID mismatch,
    /// counter regression, and so on). Wraps whatever webauthn-rs-core
    /// reports so it ends up in the server log; the client only ever sees
    /// the generic 400 below.
    #[error("Passkey ceremony failed: {0:?}")]
    Webauthn(#[from] webauthn_rs_core::error::WebauthnError),

    /// The ceremony id a `/finish` call named isn't in the in-memory
    /// table - already consumed, or never existed (e.g. a stale tab).
    #[error("Unknown or already-used passkey ceremony")]
    CeremonyNotFound,

    /// The ceremony id was found but its two-minute TTL had already
    /// elapsed; distinct from `CeremonyNotFound` so the client can tell
    /// "start over" from "that response was rejected".
    #[error("Passkey ceremony expired")]
    CeremonyExpired,

    /// A `/login/finish` assertion carried a credential id with no
    /// matching account - the passkey equivalent of a bad username, kept
    /// separate from `InvalidCredentials` (which is about a wrong
    /// *password*) so server logs distinguish the two auth methods.
    #[error("No account for that passkey")]
    UnknownPasskey,

    /// Passkey sign-in is a genuinely optional feature (see
    /// `passkey::build_webauthn`) - this deployment has no `HWAITING_RP_ID`/
    /// `HWAITING_RP_ORIGINS` configured, so every passkey endpoint that would need
    /// the WebAuthn machinery returns this instead of panicking or
    /// pretending to work.
    #[error("Passkey sign-in is not configured on this server")]
    PasskeysDisabled,

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
            AppError::Webauthn(ref e) => {
                eprintln!("Passkey ceremony failed: {:?}", e);
                (StatusCode::BAD_REQUEST, "Passkey ceremony failed".to_string())
            }
            AppError::CeremonyNotFound => {
                (StatusCode::BAD_REQUEST, "Unknown or already-used passkey ceremony".to_string())
            }
            AppError::CeremonyExpired => {
                (StatusCode::GONE, "Passkey ceremony expired, please try again".to_string())
            }
            AppError::UnknownPasskey => {
                (StatusCode::UNAUTHORIZED, "No account for that passkey".to_string())
            }
            AppError::PasskeysDisabled => {
                (StatusCode::NOT_IMPLEMENTED, "Passkey sign-in is not configured on this server".to_string())
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
        match <Json<T> as FromRequest<S>>::from_request(req, state).await {
            Ok(Json(value)) => Ok(AppJson(value)),
            Err(rejection) => Err(AppError::BadRequest(rejection.body_text())),
        }
    }
}

/// Lets `Option<AppJson<T>>` be used as an extractor, for endpoints where the
/// body itself is optional (as opposed to merely having optional fields
/// within it). Mirrors `Json<T>`'s own `OptionalFromRequest` impl: `None`
/// only when the request has no `Content-Type` header at all - a body sent
/// with `Content-Type: application/json` still has to be valid JSON (`{}` at
/// minimum), empty bytes included.
impl<S, T> OptionalFromRequest<S> for AppJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Option<Self>, Self::Rejection> {
        match <Json<T> as OptionalFromRequest<S>>::from_request(req, state).await {
            Ok(Some(Json(value))) => Ok(Some(AppJson(value))),
            Ok(None) => Ok(None),
            Err(rejection) => Err(AppError::BadRequest(rejection.body_text())),
        }
    }
}

/// Drop-in replacement for `axum::Path` as a request extractor. Same
/// conversion as `AppJson`, for URL path-segment parsing failures (e.g. a
/// non-numeric `{card_id}`).
pub struct AppPath<T>(pub T);

impl<S, T> FromRequestParts<S> for AppPath<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Path::<T>::from_request_parts(parts, state).await {
            Ok(Path(value)) => Ok(AppPath(value)),
            Err(rejection) => Err(AppError::BadRequest(rejection.body_text())),
        }
    }
}

/// Drop-in replacement for `axum::Query` as a request extractor. Same
/// conversion as `AppJson`, for query-string parsing failures (e.g. a
/// non-numeric `?exclude=`, or a missing required param).
pub struct AppQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for AppQuery<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(Query(value)) => Ok(AppQuery(value)),
            Err(rejection) => Err(AppError::BadRequest(rejection.body_text())),
        }
    }
}
