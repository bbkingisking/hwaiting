//! WebAuthn passkey sign-up/sign-in, alongside (not instead of) the
//! username/password auth in `auth.rs`. To the database and to the rest of
//! the backend this is just a second way to end up with a JWT: it stores a
//! per-user WebAuthn handle and a table of credentials, and otherwise defers
//! entirely to `auth::generate_token`.
//!
//! Every ceremony (`/register/*`, `/login/*`) is two calls: `start` builds
//! the options the browser needs and stashes server-side ceremony state
//! in-memory under a fresh id; `finish` takes that id back plus the
//! browser's response, verifies it, and either issues a JWT or - for the
//! authenticated "add a passkey to my account" ceremonies - attaches a
//! credential to the caller's account.
//!
//! Two buttons, no identifiers: `register` and `login` are separate actions
//! the frontend wires to separate buttons, not a single "enter" flow that
//! tries one then falls back to the other (that was prototyped and felt
//! worse - see the session this module was written in).
//!
//! Sign-in is a *discoverable*-credential assertion: `login/start` sends an
//! empty allow-list, and the browser's own passkey picker lists every
//! credential registered for this RP ID. That's why every passkey is
//! registered with `require_resident_key(true)` - a non-discoverable
//! credential would be unreachable from this flow, since there is no
//! identifier to look one up by.

use axum::{
    extract::{FromRef, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::{info, warn};
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;
use webauthn_rs_core::{
    proto::{
        AttestationConveyancePreference, AuthenticationState, COSEAlgorithm,
        CreationChallengeResponse, CredProtect, Credential, CredentialID,
        CredentialProtectionPolicy, PublicKeyCredential, RegisterPublicKeyCredential,
        RegistrationState, RequestAuthenticationExtensions, RequestChallengeResponse,
        RequestRegistrationExtensions, UserVerificationPolicy,
    },
    WebauthnCore,
};

use crate::auth::{generate_token, AuthResponse, AuthUser};
use crate::error::{AppError, AppJson, AppPath};

/// How long a `start` ceremony's state is kept before `finish` must have
/// used it. Long enough for a user to pick a device in the OS passkey
/// picker; short enough that a stale one isn't worth cleaning up on any
/// schedule more elaborate than "sweep expired entries on next lookup".
const CEREMONY_TTL: Duration = Duration::from_secs(120);

/// The spec requires a non-empty WebAuthn `user.name`/`displayName`, and
/// this app deliberately stores no identifying field to put there instead -
/// see the plan this module implements. A constant is the least
/// identifying value that satisfies the requirement; the cost is that two
/// accounts registered from the same device are indistinguishable in the
/// OS's own picker UI, which is an acceptable trade for "no identifiers at
/// all".
const USER_LABEL: &str = "hwaiting";

enum Ceremony {
    /// `for_user: None` is a public `/api/auth/passkey/register/*`
    /// ceremony - finishing it creates a new account. `Some(user_id)` is an
    /// authenticated `/api/user/passkeys/register/*` ceremony adding a
    /// passkey to an already-signed-in account; `finish` re-checks the
    /// caller's JWT still names that same user before writing anything.
    Registration {
        handle: Uuid,
        for_user: Option<i64>,
        state: RegistrationState,
        expires: Instant,
    },
    Authentication {
        state: AuthenticationState,
        expires: Instant,
    },
}

impl Ceremony {
    fn expired(&self) -> bool {
        let expires = match self {
            Ceremony::Registration { expires, .. } | Ceremony::Authentication { expires, .. } => {
                *expires
            }
        };
        Instant::now() > expires
    }
}

/// Router state. `pool` is `pub` and re-exposed to `SqlitePool` via the
/// `FromRef` impl below purely so every existing handler (`cards`, `admin`,
/// `user`, `export_import`, ...) keeps its `State<SqlitePool>` extractor
/// unchanged - they never needed to learn this module exists.
///
/// `Clone` (axum requires the router's state type to be `Clone`) is cheap:
/// `SqlitePool` is itself an `Arc`-backed handle, and `webauthn`/
/// `ceremonies` are wrapped the same way, so cloning `AppState` is just
/// bumping three refcounts, not copying the ceremony table.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    webauthn: Arc<WebauthnCore>,
    ceremonies: Arc<Mutex<HashMap<Uuid, Ceremony>>>,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            webauthn: Arc::new(build_webauthn()),
            ceremonies: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn store_ceremony(&self, ceremony: Ceremony) -> Uuid {
        let id = Uuid::new_v4();
        self.ceremonies.lock().unwrap().insert(id, ceremony);
        id
    }

    /// Removes and returns the ceremony, opportunistically sweeping every
    /// other expired entry while the lock is held. Consuming on read (not
    /// just on success) means a `finish` that fails validation can't be
    /// retried against the same ceremony state, which mirrors the
    /// single-use nature of the challenge itself.
    fn take_ceremony(&self, id: Uuid) -> Result<Ceremony, AppError> {
        let mut map = self.ceremonies.lock().unwrap();
        map.retain(|_, c| !c.expired());
        map.remove(&id).ok_or(AppError::CeremonyNotFound)
    }
}

impl FromRef<AppState> for SqlitePool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

/// Reads `RP_ID` (a hostname - WebAuthn's "relying party id") and
/// `RP_ORIGINS` (comma-separated full origins the frontend is served from,
/// e.g. `https://hwaiting.example.com`) the same way `main.rs` reads `HOST`
/// and `PORT`: required, and a panic at startup on anything wrong rather
/// than a confusing runtime failure on the first ceremony. WebAuthn only
/// runs in a secure context, so in production these must be the app's real
/// HTTPS hostname and origin, not the bare `HOST`/`PORT` this binary listens
/// on internally behind a TLS-terminating proxy.
fn build_webauthn() -> WebauthnCore {
    let rp_id = env::var("RP_ID").expect("RP_ID environment variable must be set");
    let origins: Vec<Url> = env::var("RP_ORIGINS")
        .expect("RP_ORIGINS environment variable must be set")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            Url::parse(s).unwrap_or_else(|e| {
                panic!("Invalid origin '{s}' in RP_ORIGINS: {e}")
            })
        })
        .collect();
    if origins.is_empty() {
        panic!("RP_ORIGINS must contain at least one origin");
    }

    WebauthnCore::new_unsafe_experts_only(
        "hwaiting",
        &rp_id,
        origins,
        CEREMONY_TTL,
        Some(false),
        Some(false),
    )
}

// ---------------------------------------------------------------- wire types

#[derive(Serialize)]
pub struct StartResponse<T: Serialize> {
    ceremony_id: Uuid,
    options: T,
}

#[derive(Deserialize)]
pub struct FinishRequest<T> {
    ceremony_id: Uuid,
    credential: T,
}

#[derive(Serialize, ToSchema)]
pub struct PasskeySummary {
    pub id: i64,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ListPasskeysResponse {
    pub passkeys: Vec<PasskeySummary>,
}

// ---------------------------------------------------------------- shared registration logic

/// Builds the creation options both registration ceremonies (public
/// sign-up, and an authenticated user adding a second device) share; they
/// differ only in whose `handle` is used, what `for_user` gets recorded as,
/// and which existing credential ids are excluded so a device can't
/// register the same authenticator twice.
fn start_registration(
    state: &AppState,
    handle: Uuid,
    for_user: Option<i64>,
    exclude_credentials: Option<Vec<CredentialID>>,
) -> Result<(Uuid, CreationChallengeResponse), AppError> {
    let extensions = RequestRegistrationExtensions {
        cred_protect: Some(CredProtect {
            credential_protection_policy: CredentialProtectionPolicy::UserVerificationRequired,
            enforce_credential_protection_policy: Some(false),
        }),
        uvm: Some(true),
        cred_props: Some(true),
        min_pin_length: None,
        hmac_create_secret: None,
    };

    let builder = state
        .webauthn
        .new_challenge_register_builder(handle.as_bytes(), USER_LABEL, USER_LABEL)?
        .attestation(AttestationConveyancePreference::None)
        .credential_algorithms(COSEAlgorithm::secure_algs())
        // Mandatory: login/start sends an empty allow-list, so a
        // non-discoverable credential would never be found.
        .require_resident_key(true)
        .authenticator_attachment(None)
        .user_verification_policy(UserVerificationPolicy::Required)
        .reject_synchronised_authenticators(false)
        .exclude_credentials(exclude_credentials)
        .hints(None)
        .extensions(Some(extensions));
    let (options, reg_state) = state.webauthn.generate_challenge_register(builder)?;

    let ceremony_id = state.store_ceremony(Ceremony::Registration {
        handle,
        for_user,
        state: reg_state,
        expires: Instant::now() + CEREMONY_TTL,
    });
    Ok((ceremony_id, options))
}

/// Verifies a registration response and persists the resulting credential.
/// Returns the id of the user it now belongs to (freshly created, for a
/// public sign-up ceremony) plus whether that user was just created -
/// callers use that to decide between "issue a JWT" and "attach to the
/// already-authenticated caller".
async fn finish_registration(
    state: &AppState,
    ceremony_id: Uuid,
    credential: &RegisterPublicKeyCredential,
) -> Result<(i64, Ceremony), AppError> {
    let ceremony = state.take_ceremony(ceremony_id)?;
    if ceremony.expired() {
        return Err(AppError::CeremonyExpired);
    }
    // Field renamed on binding (`state: ref reg_state`) so it doesn't shadow
    // the outer `state: &AppState` parameter, which is still needed below.
    let Ceremony::Registration { state: ref reg_state, .. } = ceremony else {
        return Err(AppError::CeremonyNotFound);
    };

    let cred = state.webauthn.register_credential(credential, reg_state, None)?;

    // One transaction for account creation (or reuse) and the passkey row
    // itself, so a failure partway through never leaves one without the
    // other.
    let mut tx = state.pool.begin().await?;

    let user_id = match &ceremony {
        Ceremony::Registration { for_user: Some(uid), .. } => *uid,
        Ceremony::Registration { handle, .. } => {
            let result = sqlx::query("INSERT INTO users (handle) VALUES (?)")
                .bind(handle.as_bytes().as_slice())
                .execute(&mut *tx)
                .await?;
            result.last_insert_rowid()
        }
        Ceremony::Authentication { .. } => unreachable!("take_ceremony returned the wrong variant"),
    };

    let credential_json = serde_json::to_string(&cred)
        .map_err(|e| AppError::Internal(format!("failed to serialize passkey credential: {e}")))?;
    sqlx::query("INSERT INTO passkeys (user_id, credential_id, credential) VALUES (?, ?, ?)")
        .bind(user_id)
        .bind(cred.cred_id.as_slice())
        .bind(credential_json)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok((user_id, ceremony))
}

// ---------------------------------------------------------------- public: sign-up

#[utoipa::path(
    post,
    path = "/api/auth/passkey/register/start",
    responses(
        (status = 200, description = "WebAuthn creation options for a new passkey account. \
            Opaque to OpenAPI - pass straight to navigator.credentials.create() after \
            base64url-decoding challenge/user.id."),
    ),
    tag = "auth"
)]
pub async fn register_start(
    State(state): State<AppState>,
) -> Result<Json<StartResponse<CreationChallengeResponse>>, AppError> {
    let handle = Uuid::new_v4();
    let (ceremony_id, options) = start_registration(&state, handle, None, None)?;
    info!("passkey register/start: ceremony={ceremony_id}");
    Ok(Json(StartResponse { ceremony_id, options }))
}

#[utoipa::path(
    post,
    path = "/api/auth/passkey/register/finish",
    responses(
        (status = 201, description = "Account created from the verified passkey", body = AuthResponse),
        (status = 400, description = "Ceremony verification failed, or unknown ceremony id", body = crate::error::ErrorResponse),
        (status = 410, description = "Ceremony expired", body = crate::error::ErrorResponse),
    ),
    tag = "auth"
)]
pub async fn register_finish(
    State(state): State<AppState>,
    AppJson(req): AppJson<FinishRequest<RegisterPublicKeyCredential>>,
) -> Result<(StatusCode, Json<AuthResponse>), AppError> {
    let (user_id, _ceremony) = finish_registration(&state, req.ceremony_id, &req.credential).await?;
    info!("passkey register/finish: created user_id={user_id}");

    let token = generate_token(user_id)?;
    Ok((
        StatusCode::CREATED,
        Json(AuthResponse { token, username: None, is_admin: false }),
    ))
}

// ---------------------------------------------------------------- public: sign-in

#[utoipa::path(
    post,
    path = "/api/auth/passkey/login/start",
    responses(
        (status = 200, description = "WebAuthn request options for a discoverable-credential \
            sign-in (empty allowCredentials - the browser's own picker lists every passkey \
            registered for this site). Opaque to OpenAPI."),
    ),
    tag = "auth"
)]
pub async fn login_start(
    State(state): State<AppState>,
) -> Result<Json<StartResponse<RequestChallengeResponse>>, AppError> {
    let builder = state
        .webauthn
        .new_challenge_authenticate_builder(Vec::new(), Some(UserVerificationPolicy::Required))?
        .extensions(Some(RequestAuthenticationExtensions {
            appid: None,
            uvm: Some(true),
            hmac_get_secret: None,
        }))
        .allow_backup_eligible_upgrade(false)
        .hints(None);
    let (options, auth_state) = state.webauthn.generate_challenge_authenticate(builder)?;

    let ceremony_id = state.store_ceremony(Ceremony::Authentication {
        state: auth_state,
        expires: Instant::now() + CEREMONY_TTL,
    });
    info!("passkey login/start: ceremony={ceremony_id}");
    Ok(Json(StartResponse { ceremony_id, options }))
}

#[utoipa::path(
    post,
    path = "/api/auth/passkey/login/finish",
    responses(
        (status = 200, description = "Signed in", body = AuthResponse),
        (status = 400, description = "Ceremony verification failed, or unknown ceremony id", body = crate::error::ErrorResponse),
        (status = 401, description = "No account matches that passkey", body = crate::error::ErrorResponse),
        (status = 410, description = "Ceremony expired", body = crate::error::ErrorResponse),
    ),
    tag = "auth"
)]
pub async fn login_finish(
    State(state): State<AppState>,
    AppJson(req): AppJson<FinishRequest<PublicKeyCredential>>,
) -> Result<Json<AuthResponse>, AppError> {
    let ceremony = state.take_ceremony(req.ceremony_id)?;
    if ceremony.expired() {
        return Err(AppError::CeremonyExpired);
    }
    let Ceremony::Authentication { state: mut auth_state, .. } = ceremony else {
        return Err(AppError::CeremonyNotFound);
    };

    // The assertion carries the WebAuthn user handle we generated at
    // registration - that, not any identifier, is how the account is found.
    let handle = req
        .credential
        .get_user_unique_id()
        .ok_or(AppError::UnknownPasskey)?;

    let user_row = sqlx::query("SELECT id, is_admin FROM users WHERE handle = ?")
        .bind(handle)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::UnknownPasskey)?;
    let user_id: i64 = user_row.get("id");
    let is_admin: bool = user_row.get("is_admin");

    let rows = sqlx::query("SELECT id, credential FROM passkeys WHERE user_id = ?")
        .bind(user_id)
        .fetch_all(&state.pool)
        .await?;
    let mut creds: Vec<(i64, Credential)> = rows
        .into_iter()
        .map(|row| {
            let passkey_id: i64 = row.get("id");
            let credential_json: String = row.get("credential");
            let cred: Credential = serde_json::from_str(&credential_json).map_err(|e| {
                AppError::Internal(format!("failed to deserialize stored passkey credential: {e}"))
            })?;
            Ok((passkey_id, cred))
        })
        .collect::<Result<_, AppError>>()?;
    if creds.is_empty() {
        warn!("login/finish: handle matched user_id={user_id} but it has no passkeys");
        return Err(AppError::UnknownPasskey);
    }

    auth_state.set_allowed_credentials(creds.iter().map(|(_, c)| c.clone()).collect());
    let result = state.webauthn.authenticate_credential(&req.credential, &auth_state)?;

    // Persist the updated signature counter and backup flags - this is the
    // clone-detection state webauthn-rs expects the relying party to keep.
    if let Some((passkey_id, cred)) = creds.iter_mut().find(|(_, c)| c.cred_id == *result.cred_id()) {
        if result.counter() > cred.counter {
            cred.counter = result.counter();
        }
        cred.backup_state = result.backup_state();
        if result.backup_eligible() {
            cred.backup_eligible = true;
        }
        let credential_json = serde_json::to_string(cred).map_err(|e| {
            AppError::Internal(format!("failed to serialize passkey credential: {e}"))
        })?;
        sqlx::query("UPDATE passkeys SET credential = ?, last_used_at = datetime('now') WHERE id = ?")
            .bind(credential_json)
            .bind(*passkey_id)
            .execute(&state.pool)
            .await?;
    }

    info!("passkey login/finish: user_id={user_id} signed in");
    let token = generate_token(user_id)?;
    Ok(Json(AuthResponse { token, username: None, is_admin }))
}

// ---------------------------------------------------------------- authenticated: manage passkeys

#[utoipa::path(
    get,
    path = "/api/user/passkeys",
    responses(
        (status = 200, description = "The caller's registered passkeys", body = ListPasskeysResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "user"
)]
pub async fn list_passkeys(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<ListPasskeysResponse>, AppError> {
    let rows = sqlx::query("SELECT id, created_at, last_used_at FROM passkeys WHERE user_id = ? ORDER BY id ASC")
        .bind(auth.0)
        .fetch_all(&state.pool)
        .await?;

    let passkeys = rows
        .into_iter()
        .map(|row| PasskeySummary {
            id: row.get("id"),
            created_at: row.get("created_at"),
            last_used_at: row.get("last_used_at"),
        })
        .collect();

    Ok(Json(ListPasskeysResponse { passkeys }))
}

#[utoipa::path(
    post,
    path = "/api/user/passkeys/register/start",
    responses(
        (status = 200, description = "WebAuthn creation options for adding a passkey to the \
            caller's existing account. Opaque to OpenAPI."),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "user"
)]
pub async fn add_passkey_start(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<StartResponse<CreationChallengeResponse>>, AppError> {
    let handle_bytes: Vec<u8> = sqlx::query_scalar("SELECT handle FROM users WHERE id = ?")
        .bind(auth.0)
        .fetch_one(&state.pool)
        .await?;
    let handle = Uuid::from_slice(&handle_bytes)
        .map_err(|e| AppError::Internal(format!("stored handle is not 16 bytes: {e}")))?;

    let existing: Vec<Vec<u8>> = sqlx::query_scalar("SELECT credential_id FROM passkeys WHERE user_id = ?")
        .bind(auth.0)
        .fetch_all(&state.pool)
        .await?;
    let exclude_credentials = (!existing.is_empty())
        .then(|| existing.into_iter().map(CredentialID::from).collect());

    let (ceremony_id, options) = start_registration(&state, handle, Some(auth.0), exclude_credentials)?;
    info!("passkey add/start: user_id={} ceremony={ceremony_id}", auth.0);
    Ok(Json(StartResponse { ceremony_id, options }))
}

#[utoipa::path(
    post,
    path = "/api/user/passkeys/register/finish",
    responses(
        (status = 201, description = "Passkey added to the caller's account", body = PasskeySummary),
        (status = 400, description = "Ceremony verification failed, or unknown ceremony id", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 403, description = "Ceremony belongs to a different account", body = crate::error::ErrorResponse),
        (status = 410, description = "Ceremony expired", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "user"
)]
pub async fn add_passkey_finish(
    State(state): State<AppState>,
    auth: AuthUser,
    AppJson(req): AppJson<FinishRequest<RegisterPublicKeyCredential>>,
) -> Result<(StatusCode, Json<PasskeySummary>), AppError> {
    let (user_id, ceremony) = finish_registration(&state, req.ceremony_id, &req.credential).await?;
    let for_user = match ceremony {
        Ceremony::Registration { for_user, .. } => for_user,
        Ceremony::Authentication { .. } => None,
    };
    if for_user != Some(auth.0) {
        warn!("passkey add/finish: ceremony for user_id={for_user:?} finished by user_id={}", auth.0);
        return Err(AppError::Forbidden);
    }

    let row = sqlx::query("SELECT id, created_at, last_used_at FROM passkeys WHERE user_id = ? ORDER BY id DESC LIMIT 1")
        .bind(user_id)
        .fetch_one(&state.pool)
        .await?;

    info!("passkey add/finish: user_id={user_id} added passkey id={}", row.get::<i64, _>("id"));
    Ok((
        StatusCode::CREATED,
        Json(PasskeySummary {
            id: row.get("id"),
            created_at: row.get("created_at"),
            last_used_at: row.get("last_used_at"),
        }),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/user/passkeys/{passkey_id}",
    responses(
        (status = 204, description = "Passkey removed"),
        (status = 400, description = "That passkey is the caller's only sign-in method", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 404, description = "No such passkey on the caller's account", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "user"
)]
pub async fn delete_passkey(
    State(state): State<AppState>,
    auth: AuthUser,
    AppPath(passkey_id): AppPath<i64>,
) -> Result<StatusCode, AppError> {
    // A password gives the account a fallback sign-in method, so only
    // block deleting the last passkey when it's the *only* one.
    let has_password: bool = sqlx::query_scalar("SELECT password_hash IS NOT NULL FROM users WHERE id = ?")
        .bind(auth.0)
        .fetch_one(&state.pool)
        .await?;
    if !has_password {
        let passkey_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE user_id = ?")
            .bind(auth.0)
            .fetch_one(&state.pool)
            .await?;
        if passkey_count <= 1 {
            return Err(AppError::BadRequest(
                "Cannot delete your only sign-in method".to_string(),
            ));
        }
    }

    let result = sqlx::query("DELETE FROM passkeys WHERE id = ? AND user_id = ?")
        .bind(passkey_id)
        .bind(auth.0)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    info!("passkey delete: user_id={} removed passkey id={passkey_id}", auth.0);
    Ok(StatusCode::NO_CONTENT)
}

