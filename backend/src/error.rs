use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, OptionalFromRequest, Path, Query, Request},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use serde::{Serialize, de::DeserializeOwned};
use tracing::error;
use utoipa::ToSchema;

/// Envelope for every JSON error response. Branch on `code`; `error` is
/// human-readable text whose wording may change.
#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    /// Human-readable message, suitable for display.
    pub error: String,
    pub code: ErrorCode,
}

/// Stable machine-readable error code. Each code always comes with the same
/// HTTP status:
///
/// - 400 `invalid_request`: malformed body, path, or query, or a failed validation rule
/// - 400 `translation_language_not_configured`: choose a translation language in settings first
/// - 400 `unknown_translation_language`: the code is well-formed but not an available language
/// - 400 `invalid_translation_language`: not a lowercase three-letter code
/// - 400 `passkey_ceremony_failed`: the authenticator response failed verification
/// - 400 `passkey_ceremony_not_found`: unknown or already-used ceremony id; start over
/// - 401 `unauthenticated`: missing, invalid, or expired authentication cookie
/// - 401 `invalid_credentials`: wrong username or password
/// - 401 `unknown_passkey`: no account owns the presented passkey
/// - 403 `csrf_header_missing`: a write request lacks `X-Requested-With: hwaiting`
/// - 403 `admin_required`: authenticated, but not an administrator
/// - 403 `signups_disabled`: this deployment does not allow new accounts
/// - 403 `passkey_ceremony_account_mismatch`: the ceremony was started by a different account
/// - 404 `not_found`: the resource does not exist or is not visible to the caller
/// - 409 `username_exists`: that username is taken
/// - 409 `card_suppressed`: the card was suppressed after it was fetched
/// - 409 `stale_review`: the card's review state changed since it was fetched
/// - 409 `stale_content`: the card's content changed since it was fetched
/// - 409 `stale_prompt`: the sentence selected for the card changed since it was fetched
/// - 409 `last_sign_in_method`: removing this passkey would leave the account with no way to sign in
/// - 410 `passkey_ceremony_expired`: the ceremony timed out; start over
/// - 500 `internal_error`: unexpected server failure
/// - 501 `passkeys_disabled`: passkey sign-in is not configured on this server
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    TranslationLanguageNotConfigured,
    UnknownTranslationLanguage,
    InvalidTranslationLanguage,
    PasskeyCeremonyFailed,
    PasskeyCeremonyNotFound,
    Unauthenticated,
    InvalidCredentials,
    UnknownPasskey,
    CsrfHeaderMissing,
    AdminRequired,
    SignupsDisabled,
    PasskeyCeremonyAccountMismatch,
    NotFound,
    UsernameExists,
    CardSuppressed,
    StaleReview,
    StaleContent,
    StalePrompt,
    LastSignInMethod,
    PasskeyCeremonyExpired,
    InternalError,
    PasskeysDisabled,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("No translation language configured")]
    TranslationLanguageNotConfigured,
    #[error("Unknown translation-language code")]
    UnknownTranslationLanguage,
    #[error("Invalid translation-language code")]
    InvalidTranslationLanguage,

    #[error("Card review state has changed")]
    StaleReview,
    #[error("Card content has changed")]
    StaleContent,
    #[error("The selected sentence has changed")]
    StalePrompt,
    #[error("Card is suppressed")]
    CardSuppressed,

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Password hashing error")]
    PasswordHash,

    /// Missing, malformed, or expired authentication cookie.
    #[error("Not authenticated")]
    Unauthenticated,

    /// Wrong username or password at login.
    #[error("Invalid credentials")]
    InvalidCredentials,

    /// A `navigator.credentials.create()`/`.get()` ceremony failed
    /// server-side verification (bad signature, origin/RP ID mismatch,
    /// counter regression, and so on). `webauthn_rp` has several disjoint
    /// error enums rather than one unified type, so this carries a formatted
    /// message for the server log; the client only sees a generic 400.
    #[error("Passkey ceremony failed: {0}")]
    Webauthn(String),

    /// The ceremony id a `/finish` call named isn't in the in-memory
    /// table - already consumed, or never existed (e.g. a stale tab).
    #[error("Unknown or already-used passkey ceremony")]
    CeremonyNotFound,

    /// The ceremony id was found but its TTL had already elapsed.
    #[error("Passkey ceremony expired")]
    CeremonyExpired,

    /// A registration ceremony started by one account was finished by another.
    #[error("Passkey ceremony belongs to a different account")]
    CeremonyAccountMismatch,

    /// A `/login/finish` assertion carried a credential id with no
    /// matching account.
    #[error("No account for that passkey")]
    UnknownPasskey,

    /// This deployment has no `HWAITING_RP_ID`/`HWAITING_RP_ORIGINS`
    /// configured, so passkey endpoints that need WebAuthn return this.
    #[error("Passkey sign-in is not configured on this server")]
    PasskeysDisabled,

    #[error("Cannot remove the account's only sign-in method")]
    LastSignInMethod,

    #[error("Sign-ups are disabled on this server")]
    SignupsDisabled,

    #[error("Username already exists")]
    UsernameExists,

    #[error("Missing or invalid X-Requested-With header")]
    CsrfHeaderMissing,

    #[error("Admin access required")]
    AdminRequired,

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

impl AppError {
    /// Status, code, and client-facing message. Logs server-side details for
    /// failures whose message is deliberately generic.
    fn parts(self) -> (StatusCode, ErrorCode, String) {
        use ErrorCode as C;
        use StatusCode as S;
        let fixed = |status, code, message: &str| (status, code, message.to_string());
        match self {
            AppError::TranslationLanguageNotConfigured => fixed(
                S::BAD_REQUEST,
                C::TranslationLanguageNotConfigured,
                "Choose a preferred translation language in your settings",
            ),
            AppError::UnknownTranslationLanguage => fixed(
                S::BAD_REQUEST,
                C::UnknownTranslationLanguage,
                "Unknown translation-language code",
            ),
            AppError::InvalidTranslationLanguage => fixed(
                S::BAD_REQUEST,
                C::InvalidTranslationLanguage,
                "Expected a lowercase three-letter translation language code",
            ),
            AppError::StaleReview => {
                fixed(S::CONFLICT, C::StaleReview, "Card review state has changed")
            }
            AppError::StaleContent => {
                fixed(S::CONFLICT, C::StaleContent, "Card content has changed")
            }
            AppError::StalePrompt => fixed(
                S::CONFLICT,
                C::StalePrompt,
                "The selected sentence has changed",
            ),
            AppError::CardSuppressed => fixed(S::CONFLICT, C::CardSuppressed, "Card is suppressed"),
            AppError::Database(e) => {
                error!("Database error: {}", e);
                fixed(S::INTERNAL_SERVER_ERROR, C::InternalError, "Database error")
            }
            AppError::PasswordHash => {
                error!("Password hash error");
                fixed(
                    S::INTERNAL_SERVER_ERROR,
                    C::InternalError,
                    "Authentication error",
                )
            }
            AppError::Unauthenticated => {
                fixed(S::UNAUTHORIZED, C::Unauthenticated, "Not authenticated")
            }
            AppError::InvalidCredentials => fixed(
                S::UNAUTHORIZED,
                C::InvalidCredentials,
                "Invalid credentials",
            ),
            AppError::Webauthn(e) => {
                error!("Passkey ceremony failed: {:?}", e);
                fixed(
                    S::BAD_REQUEST,
                    C::PasskeyCeremonyFailed,
                    "Passkey ceremony failed",
                )
            }
            AppError::CeremonyNotFound => fixed(
                S::BAD_REQUEST,
                C::PasskeyCeremonyNotFound,
                "Unknown or already-used passkey ceremony",
            ),
            AppError::CeremonyExpired => fixed(
                S::GONE,
                C::PasskeyCeremonyExpired,
                "Passkey ceremony expired, please try again",
            ),
            AppError::CeremonyAccountMismatch => fixed(
                S::FORBIDDEN,
                C::PasskeyCeremonyAccountMismatch,
                "Passkey ceremony belongs to a different account",
            ),
            AppError::UnknownPasskey => fixed(
                S::UNAUTHORIZED,
                C::UnknownPasskey,
                "No account for that passkey",
            ),
            AppError::PasskeysDisabled => fixed(
                S::NOT_IMPLEMENTED,
                C::PasskeysDisabled,
                "Passkey sign-in is not configured on this server",
            ),
            AppError::LastSignInMethod => fixed(
                S::CONFLICT,
                C::LastSignInMethod,
                "Cannot remove the account's only sign-in method",
            ),
            AppError::UsernameExists => {
                fixed(S::CONFLICT, C::UsernameExists, "Username already exists")
            }
            AppError::SignupsDisabled => fixed(
                S::FORBIDDEN,
                C::SignupsDisabled,
                "Sign-ups are disabled on this server",
            ),
            AppError::CsrfHeaderMissing => fixed(
                S::FORBIDDEN,
                C::CsrfHeaderMissing,
                "Missing or invalid X-Requested-With header",
            ),
            AppError::AdminRequired => {
                fixed(S::FORBIDDEN, C::AdminRequired, "Admin access required")
            }
            AppError::NotFound => fixed(S::NOT_FOUND, C::NotFound, "Not found"),
            AppError::BadRequest(msg) => (S::BAD_REQUEST, C::InvalidRequest, msg),
            AppError::Internal(msg) => {
                error!("Internal error: {}", msg);
                fixed(
                    S::INTERNAL_SERVER_ERROR,
                    C::InternalError,
                    "Internal server error",
                )
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, error) = self.parts();
        (status, Json(ErrorResponse { error, code })).into_response()
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn rendered(error: AppError) -> (StatusCode, serde_json::Value) {
        let response = error.into_response();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn every_error_carries_a_code_and_formerly_ambiguous_403s_are_distinct() {
        let cases = [
            (AppError::CsrfHeaderMissing, 403, "csrf_header_missing"),
            (AppError::AdminRequired, 403, "admin_required"),
            (AppError::SignupsDisabled, 403, "signups_disabled"),
            (
                AppError::CeremonyAccountMismatch,
                403,
                "passkey_ceremony_account_mismatch",
            ),
            (AppError::CardSuppressed, 409, "card_suppressed"),
            (AppError::StaleReview, 409, "stale_review"),
            (AppError::LastSignInMethod, 409, "last_sign_in_method"),
            (AppError::Unauthenticated, 401, "unauthenticated"),
            (AppError::InvalidCredentials, 401, "invalid_credentials"),
            (AppError::BadRequest("nope".into()), 400, "invalid_request"),
            (
                AppError::TranslationLanguageNotConfigured,
                400,
                "translation_language_not_configured",
            ),
            (
                AppError::Internal("secret detail".into()),
                500,
                "internal_error",
            ),
        ];
        for (error, status, code) in cases {
            let (actual_status, body) = rendered(error).await;
            assert_eq!(actual_status.as_u16(), status, "{code}");
            assert_eq!(body["code"], code);
            assert!(body["error"].is_string(), "{code}");
        }
        let (_, body) = rendered(AppError::Internal("secret detail".into())).await;
        assert!(!body["error"].as_str().unwrap().contains("secret"));
        let (_, body) = rendered(AppError::BadRequest("nope".into())).await;
        assert_eq!(body["error"], "nope");
    }
}
