//! WebAuthn passkey sign-up/sign-in, alongside (not instead of) the
//! username/password auth in `auth.rs`. To the database and to the rest of
//! the backend this is just a second way to end up with a JWT: it stores a
//! table of credentials and otherwise defers entirely to
//! `auth::generate_token`.
//!
//! Built on `webauthn_rp`, a pure-Rust relying-party library (RustCrypto
//! primitives - p256/p384/ed25519-dalek/rsa - no OpenSSL), not
//! `webauthn-rs-core` - see the session this module was rewritten in for
//! why: `webauthn-rs-core` hard-depends on OpenSSL unconditionally, and
//! this app has no other reason to link it (TLS is terminated by Caddy in
//! front of this binary; passkey registration requests
//! `AttestationConveyancePreference::None`, so the OpenSSL-backed
//! attestation-cert-chain machinery in `webauthn-rs-core` was never
//! exercised either).
//!
//! Every ceremony still needs a WebAuthn user handle to hand the
//! authenticator (the protocol requires one), but nothing here persists
//! it - each ceremony gets a fresh throwaway handle that lives only in
//! that ceremony's in-memory state.
//!
//! `login_finish` doesn't look an account up by anything before
//! verifying - it loads every passkey on the server and tries each one's
//! public key against the assertion in turn, stopping at the first real
//! signature verification that succeeds. This *is* a naive brute force of
//! up to N signature checks, not a shortcut: with no `credential_id`
//! persisted (see `20260909000000_drop_passkey_credential_id.sql`), there
//! is nothing left to narrow the candidate set by before paying for the
//! cryptography. Identity (`user_id`, `is_admin`, which passkey row to
//! stamp `last_used_at` on) comes from whichever row's public key was the
//! one that verified, not looked up beforehand. See the session this was
//! written in for the full reasoning and the cost accounting at this
//! app's scale.
//!
//! `webauthn_rp::request::auth::DiscoverableAuthenticationServerState::verify`
//! consumes itself (by design - a ceremony should only be completable
//! once), which doesn't compose directly with trying N candidate
//! credentials against the same challenge. `login_finish` works around
//! this by `Encode`ing the ceremony state once and `Decode`ing a fresh,
//! independent copy for each candidate - a sanctioned use of the crate's
//! own `serializable_server_state` (de)serialization, not a hack: the
//! crate designed that feature for exactly "give me another instance of
//! this same state", just for the persistence use case rather than this
//! one.
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
//! Sign-in is a *discoverable*-credential assertion: `login/start` uses
//! `DiscoverableCredentialRequestOptions`, so the browser's own passkey
//! picker lists every credential registered for this RP ID. That's why
//! every passkey is registered via `PublicKeyCredentialCreationOptions::
//! passkey` (resident key required) - a non-discoverable credential would
//! be unreachable from this flow, since there is no identifier to look one
//! up by.

use axum::{
    extract::{FromRef, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::{info, warn};
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;
use webauthn_rp::{
    bin::{Decode, Encode},
    request::{
        auth::{AuthenticationVerificationOptions, DiscoverableCredentialRequestOptions},
        register::{
            Nickname, PublicKeyCredentialCreationOptions, PublicKeyCredentialUserEntity,
            RegistrationVerificationOptions, UserHandle16, Username,
        },
        AsciiDomain, RpId,
    },
    response::{
        auth::AuthenticatorData as AssertionAuthenticatorData,
        register::{CompressedPubKey, DynamicState, StaticState},
        AuthenticatorAttachment,
    },
    AuthenticatedCredential, DiscoverableAuthentication16, DiscoverableAuthenticationServerState,
    Registration, RegistrationServerState,
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
/// see the module docs. A constant is the least identifying value that
/// satisfies the requirement; the cost is that two accounts registered
/// from the same device are indistinguishable in the OS's own picker UI,
/// which is an acceptable trade for "no identifiers at all". (`webauthn_rp`
/// has its own convenience for this exact case,
/// `PublicKeyCredentialUserEntity::from(&UserHandle)`, which uses the
/// literal string `"blank"` - this app spells out its own constant instead
/// purely so the OS passkey picker shows this app's name rather than that
/// placeholder.)
const USER_LABEL: &str = "hwaiting";

/// Every passkey WebAuthn credential this app ever registers uses one of
/// these four algorithms; RSA's `Vec<u8>` (variable-length modulus) is the
/// only one that isn't a fixed-size array. This is `webauthn_rp`'s own
/// documented "concrete storage type" (see its top-level example) - not a
/// guess.
type StoredPubKey = CompressedPubKey<[u8; 32], [u8; 32], [u8; 48], Vec<u8>>;

/// A candidate credential reconstructed from one `passkeys` row for a
/// single login attempt, `'a`-tied to the assertion it's being checked
/// against (see `login_finish`).
type LoginCredential<'a> = AuthenticatedCredential<'a, 'a, 16, StoredPubKey>;

enum Ceremony {
    /// `for_user: None` is a public `/api/auth/passkey/register/*`
    /// ceremony - finishing it creates a new account. `Some(user_id)` is an
    /// authenticated `/api/user/passkeys/register/*` ceremony adding a
    /// passkey to an already-signed-in account; `finish` re-checks the
    /// caller's JWT still names that same user before writing anything.
    Registration {
        for_user: Option<i64>,
        state: RegistrationServerState<16>,
        expires: Instant,
    },
    Authentication {
        state: DiscoverableAuthenticationServerState,
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

/// Resolved `HWAITING_RP_ID`/`HWAITING_RP_ORIGINS`. `rp_id` is `webauthn_rp`'s
/// own type; `origins` deliberately isn't `webauthn_rp::request::Url` - that
/// type validates a URL with *no host* (it backs the non-domain `RpId::Url`
/// variant, for native-app RP ids expressed as a custom URL scheme) and
/// would reject every real origin here, which all have one. `verify`'s
/// `allowed_origins` only requires `PartialEq<Origin>`, which plain
/// `String` satisfies with an ordinary string compare - the same compare
/// `webauthn_rp::request::Url` itself boils down to - so a `String`
/// validated as a proper origin serves just as well without the wrong
/// constraint.
///
/// `origins` is a `Vec` (not the single origin `webauthn_rp` can derive
/// from `rp_id` alone) because this app supports multiple configured
/// origins (e.g. a prod and a demo frontend sharing one RP ID) - every
/// `verify` call passes it explicitly rather than relying on the
/// single-origin default.
struct WebauthnConfig {
    rp_id: RpId,
    origins: Vec<String>,
}

impl WebauthnConfig {
    fn allowed_origins(&self) -> Vec<&str> {
        self.origins.iter().map(String::as_str).collect()
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
    /// `None` when `HWAITING_RP_ID`/`HWAITING_RP_ORIGINS` aren't configured - passkey sign-in
    /// is an optional feature, not a required one, see `build_webauthn_config`.
    webauthn: Option<Arc<WebauthnConfig>>,
    ceremonies: Arc<Mutex<HashMap<Uuid, Ceremony>>>,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            webauthn: build_webauthn_config().map(Arc::new),
            ceremonies: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// The configured WebAuthn RP settings, or `AppError::PasskeysDisabled`
    /// if this deployment never set `HWAITING_RP_ID`/`HWAITING_RP_ORIGINS`.
    /// Every handler that needs to run an actual ceremony goes through this
    /// instead of touching the field directly.
    fn webauthn(&self) -> Result<&WebauthnConfig, AppError> {
        self.webauthn.as_deref().ok_or(AppError::PasskeysDisabled)
    }

    /// Same fact as `webauthn()`, as a plain bool for `capabilities` to
    /// report - the frontend needs to know *whether* passkeys work before
    /// it ever tries a ceremony, not just get told so after the fact.
    fn passkeys_enabled(&self) -> bool {
        self.webauthn.is_some()
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

/// Reads `HWAITING_RP_ID` (a hostname - WebAuthn's "relying party id") and
/// `HWAITING_RP_ORIGINS` (comma-separated full origins the frontend is served from,
/// e.g. `https://hwaiting.example.com`), each from a systemd credential or
/// env var (see `credentials::rp_id`/`rp_origins`).
///
/// Passkey sign-in is optional, alongside (not instead of) username/password
/// auth - see the module docs - so `None` here (both unset) just means this
/// deployment isn't using it: `AppState::new` stores no `WebauthnConfig`,
/// and every passkey endpoint returns `AppError::PasskeysDisabled` instead
/// of running a ceremony. Setting only one of the two, or an unparseable
/// `HWAITING_RP_ORIGINS`, is a config mistake rather than "half enabled" and still
/// panics at startup, same as before.
///
/// There's still no default derived from `HWAITING_HOST`/`HWAITING_PORT` when both are unset:
/// WebAuthn only runs in a secure context, and in production `HWAITING_RP_ID`/
/// `HWAITING_RP_ORIGINS` must be the app's real HTTPS hostname and origin, not the
/// bare `HWAITING_HOST`/`HWAITING_PORT` this binary listens on internally behind a
/// TLS-terminating proxy - so guessing from those would be wrong exactly
/// when it matters most, and silently so.
fn build_webauthn_config() -> Option<WebauthnConfig> {
    let (rp_id, rp_origins) = match (crate::credentials::rp_id(), crate::credentials::rp_origins()) {
        (None, None) => {
            info!("HWAITING_RP_ID/HWAITING_RP_ORIGINS not set - passkey sign-in disabled");
            return None;
        }
        (Some(rp_id), Some(rp_origins)) => (rp_id, rp_origins),
        (Some(_), None) => panic!("HWAITING_RP_ID is set but HWAITING_RP_ORIGINS is not - passkeys need both or neither"),
        (None, Some(_)) => panic!("HWAITING_RP_ORIGINS is set but HWAITING_RP_ID is not - passkeys need both or neither"),
    };

    let ascii_domain = AsciiDomain::try_from(rp_id.clone())
        .unwrap_or_else(|e| panic!("Invalid HWAITING_RP_ID {rp_id:?}: {e:?}"));

    let origins: Vec<String> = rp_origins
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| parse_origin(s).unwrap_or_else(|e| panic!("Invalid origin '{s}' in HWAITING_RP_ORIGINS: {e}")))
        .collect();
    if origins.is_empty() {
        panic!("HWAITING_RP_ORIGINS must contain at least one origin");
    }

    info!("Passkey sign-in enabled for RP ID {rp_id:?}, origins {origins:?}");
    Some(WebauthnConfig { rp_id: RpId::Domain(ascii_domain), origins })
}

/// Validates `s` as a full [origin](https://www.w3.org/TR/webauthn-3/#dom-collectedclientdata-origin) -
/// `http`/`https` scheme, a host, and nothing else (no path/query/fragment/
/// userinfo) - and returns it unchanged as an owned `String` for
/// `WebauthnConfig::origins` to compare a ceremony's `CollectedClientData::origin`
/// against by exact string equality (see `WebauthnConfig`'s doc comment for
/// why that's a plain `String` rather than `webauthn_rp::request::Url`).
///
/// Deliberately returns the input `s` itself rather than `url::Url::parse`'s
/// own serialization: that parse is used only to check the origin's shape,
/// since re-serializing would append the `/` root path `url::Url` gives
/// every hierarchical URL - which would then never match the slash-less
/// origin string a real browser sends.
fn parse_origin(s: &str) -> Result<String, String> {
    let url = Url::parse(s).map_err(|e| e.to_string())?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(format!("scheme must be http or https, got {:?}", url.scheme()));
    }
    if !url.has_host() {
        return Err("must have a host".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("must not contain userinfo".to_string());
    }
    if !matches!(url.path(), "" | "/") {
        return Err(format!("must not have a path, got {:?}", url.path()));
    }
    if url.query().is_some() {
        return Err("must not have a query".to_string());
    }
    if url.fragment().is_some() {
        return Err("must not have a fragment".to_string());
    }
    Ok(s.trim_end_matches('/').to_string())
}

// ---------------------------------------------------------------- wire types

#[derive(Serialize)]
pub struct StartResponse {
    ceremony_id: Uuid,
    /// The `PublicKeyCredentialCreationOptionsJSON`/`PublicKeyCredentialRequestOptionsJSON`
    /// `webauthn_rp` serializes its client-state types into - passed
    /// straight to `navigator.credentials.create()`/`.get()` by the
    /// frontend after base64url-decoding the binary fields. Untyped here
    /// (rather than generic like the old `webauthn-rs-core`-backed version
    /// was) because the client-state types borrow from ceremony-local data
    /// (the throwaway user handle) that doesn't outlive this function, so
    /// they're serialized to a `Value` before returning rather than
    /// threaded through the return type.
    options: serde_json::Value,
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

/// Which optional features this deployment has turned on. Just the one
/// field today - passkeys is the only config-gated optional feature that
/// matters to the frontend - but this is the endpoint any future one would
/// join, rather than each growing its own ad hoc discovery mechanism.
#[derive(Serialize, ToSchema)]
pub struct Capabilities {
    pub passkeys_enabled: bool,
}

// ---------------------------------------------------------------- public: capabilities

#[utoipa::path(
    get,
    path = "/api/capabilities",
    responses(
        (status = 200, description = "Which optional features this deployment has turned on", body = Capabilities),
    ),
    tag = "auth"
)]
pub async fn capabilities(State(state): State<AppState>) -> Json<Capabilities> {
    Json(Capabilities { passkeys_enabled: state.passkeys_enabled() })
}

// ---------------------------------------------------------------- shared registration logic

/// Builds the creation options both registration ceremonies (public
/// sign-up, and an authenticated user adding a second device) share; they
/// differ only in what `for_user` gets recorded as. The user handle is
/// thrown away once this returns - it's never read back after the
/// authenticator embeds it in the credential.
///
/// `PublicKeyCredentialCreationOptions::passkey` already requires
/// resident-key/user-verification (`AuthenticatorSelectionCriteria::
/// passkey`) and requests no attestation - exactly this app's policy,
/// without needing to hand-build a custom set of options for it.
fn start_registration(
    state: &AppState,
    for_user: Option<i64>,
) -> Result<(Uuid, serde_json::Value), AppError> {
    let config = state.webauthn()?;
    let user_handle = UserHandle16::new();
    let user = PublicKeyCredentialUserEntity {
        name: Username::try_from(USER_LABEL)
            .unwrap_or_else(|e| panic!("USER_LABEL {USER_LABEL:?} is not a valid Username: {e:?}")),
        id: &user_handle,
        display_name: Some(Nickname::try_from(USER_LABEL).unwrap_or_else(|e| {
            panic!("USER_LABEL {USER_LABEL:?} is not a valid Nickname: {e:?}")
        })),
    };
    let (reg_state, client_state) = PublicKeyCredentialCreationOptions::passkey(
        &config.rp_id,
        user,
        // Always an empty exclude-list: excluding a device already
        // registered to this account would need its credential id on hand,
        // and this app doesn't persist one (see
        // `20260909000000_drop_passkey_credential_id.sql`). So the same
        // physical authenticator *can* end up registered to one account
        // twice, as two separate rows sharing a public key - a deliberate
        // consequence of storing nothing to exclude by, not a bug.
        Vec::new(),
    )
    .start_ceremony()
    .map_err(|e| AppError::Internal(format!("failed to start registration ceremony: {e:?}")))?;

    let options = serde_json::to_value(&client_state)
        .map_err(|e| AppError::Internal(format!("failed to serialize creation options: {e}")))?;

    let ceremony_id = state.store_ceremony(Ceremony::Registration {
        for_user,
        state: reg_state,
        expires: Instant::now() + CEREMONY_TTL,
    });
    Ok((ceremony_id, options))
}

/// Verifies a registration response and persists the resulting credential.
/// Returns the id of the user it now belongs to (freshly created, for a
/// public sign-up ceremony) plus `for_user` unchanged, so callers can
/// decide between "issue a JWT" and "attach to the already-authenticated
/// caller".
async fn finish_registration(
    state: &AppState,
    ceremony_id: Uuid,
    credential: &Registration,
) -> Result<(i64, Option<i64>), AppError> {
    let ceremony = state.take_ceremony(ceremony_id)?;
    if ceremony.expired() {
        return Err(AppError::CeremonyExpired);
    }
    let Ceremony::Registration { for_user, state: reg_state, .. } = ceremony else {
        return Err(AppError::CeremonyNotFound);
    };

    let config = state.webauthn()?;
    let allowed_origins = config.allowed_origins();
    let options = RegistrationVerificationOptions::<&str, &str> {
        allowed_origins: &allowed_origins,
        ..Default::default()
    };
    let registered = reg_state
        .verify(&config.rp_id, credential, &options)
        .map_err(|e| AppError::Webauthn(format!("{e:?}")))?;

    // `webauthn_rp`'s own storage format: `StaticState::encode` compresses
    // EC public keys as it serializes (see `StoredPubKey`'s doc comment),
    // so this is already the compact on-disk representation, not a
    // temporary one that needs further massaging. Infallible - `Encode`'s
    // `Err` type for `StaticState<UncompressedPubKey>` is `Infallible`.
    let public_key_blob = registered
        .static_state()
        .encode()
        .expect("StaticState::<UncompressedPubKey>::encode is Infallible");

    // One transaction for account creation (or reuse) and the passkey row
    // itself, so a failure partway through never leaves one without the
    // other.
    let mut tx = state.pool.begin().await?;

    let user_id = match for_user {
        Some(uid) => uid,
        None => {
            let result = sqlx::query("INSERT INTO users DEFAULT VALUES")
                .execute(&mut *tx)
                .await?;
            result.last_insert_rowid()
        }
    };

    // The credential id this authenticator just generated for itself
    // during registration is deliberately discarded, not persisted: see
    // `20260909000000_drop_passkey_credential_id.sql`. Only the public key
    // survives; `login_finish` forges a credential id per login attempt
    // instead of ever storing this one.
    sqlx::query("INSERT INTO passkeys (user_id, public_key) VALUES (?, ?)")
        .bind(user_id)
        .bind(public_key_blob)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok((user_id, for_user))
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
) -> Result<Json<StartResponse>, AppError> {
    let (ceremony_id, options) = start_registration(&state, None)?;
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
    AppJson(req): AppJson<FinishRequest<webauthn_rp::response::register::ser_relaxed::RegistrationRelaxed>>,
) -> Result<(StatusCode, Json<AuthResponse>), AppError> {
    let (user_id, _for_user) = finish_registration(&state, req.ceremony_id, &req.credential.0).await?;
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
) -> Result<Json<StartResponse>, AppError> {
    let config = state.webauthn()?;
    let (auth_state, client_state) = DiscoverableCredentialRequestOptions::passkey(&config.rp_id)
        .start_ceremony()
        .map_err(|e| AppError::Internal(format!("failed to start authentication ceremony: {e:?}")))?;

    let options = serde_json::to_value(&client_state)
        .map_err(|e| AppError::Internal(format!("failed to serialize request options: {e}")))?;

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
    AppJson(req): AppJson<
        FinishRequest<webauthn_rp::response::auth::ser_relaxed::AuthenticationRelaxed<16, true>>,
    >,
) -> Result<Json<AuthResponse>, AppError> {
    let ceremony = state.take_ceremony(req.ceremony_id)?;
    if ceremony.expired() {
        return Err(AppError::CeremonyExpired);
    }
    let Ceremony::Authentication { state: auth_state, .. } = ceremony else {
        return Err(AppError::CeremonyNotFound);
    };
    let credential: DiscoverableAuthentication16 = req.credential.0;

    let config = state.webauthn()?;

    // `verify` consumes the ceremony state (by design), which doesn't
    // compose with trying N candidate credentials against the same
    // challenge - so each candidate below gets its own independently
    // `Decode`d copy of this same, once-`Encode`d state. See the module
    // docs.
    let encoded_state = auth_state
        .encode()
        .map_err(|e| AppError::Internal(format!("failed to encode ceremony state: {e:?}")))?;

    // This app doesn't persist a per-credential backup-eligibility flag
    // (there's nothing stored to compare against), so every candidate
    // below is reconstructed with *this assertion's own* backup flags
    // rather than a stored one - that's what makes the crate's
    // backup-state checks a no-op for any spec-compliant authenticator
    // instead of a permanent rejection of every synced/platform passkey.
    // Parsed with the crate's own parser directly from the assertion's
    // authenticatorData, not hand-rolled, so it can't drift from what
    // `verify` itself computes from the same bytes.
    let asserted_backup = AssertionAuthenticatorData::try_from(credential.response().authenticator_data())
        .map_err(|_| AppError::BadRequest("malformed authenticatorData".to_string()))?
        .flags()
        .backup;

    // Similarly, no `UserHandle` is persisted for any account (see the
    // module docs), so `cred.user_id` below is forged to be exactly the
    // `userHandle` this assertion itself carries - the equality check
    // `verify` runs between the two becomes self-referential, and an
    // attacker can't forge it without also forging a valid signature over
    // this same assertion.
    let user_handle = credential.response().user_handle();

    // No `credential_id` is stored (see
    // `20260909000000_drop_passkey_credential_id.sql`), so there is nothing
    // to narrow this query - or the loop below - by: every passkey on the
    // server, for every account, is a candidate. Each one is handed to
    // `verify` with its credential id forged to equal this assertion's own
    // `rawId`, which makes the crate's internal id-match step trivially
    // pass every candidate through to a real signature verification
    // against its stored public key. So this genuinely runs up to N
    // signature verifications per login attempt, not the
    // one-verify-plus-cheap-scan an indexed lookup would give - that's the
    // actual cost of not persisting an id to narrow by, not a shortcut. See
    // the session this migration was written in for the accounting of why
    // that stays fine at this app's scale.
    let rows = sqlx::query(
        "SELECT passkeys.id, passkeys.user_id, passkeys.public_key, users.is_admin \
         FROM passkeys JOIN users ON users.id = passkeys.user_id",
    )
    .fetch_all(&state.pool)
    .await?;
    let candidate_count = rows.len();

    let allowed_origins = config.allowed_origins();
    let options = AuthenticationVerificationOptions::<&str, &str> {
        allowed_origins: &allowed_origins,
        ..Default::default()
    };

    let raw_id = credential.raw_id();
    let mut matched: Option<(i64, i64, bool)> = None;
    for row in &rows {
        let public_key: Vec<u8> = row.get("public_key");
        let static_state = StaticState::<StoredPubKey>::decode(public_key.as_slice())
            .map_err(|e| AppError::Internal(format!("failed to decode stored passkey public key: {e:?}")))?;
        // No per-login state is tracked for any of these (see the module
        // docs): `user_verified: true` and `sign_count: 0` are the values
        // that make the crate's corresponding checks no-ops given this
        // app's fixed `UserVerificationRequirement::Required` policy and
        // lack of any stored counter to compare against; `backup` is the
        // self-referential value computed above;
        // `authenticator_attachment: None` is what makes the (default,
        // `Ignore`) attachment-enforcement check a no-op too.
        let dynamic_state = DynamicState {
            user_verified: true,
            backup: asserted_backup,
            sign_count: 0,
            authenticator_attachment: AuthenticatorAttachment::None,
        };
        let mut candidate: LoginCredential<'_> =
            AuthenticatedCredential::new(raw_id, user_handle, static_state, dynamic_state)
                .map_err(|e| AppError::Internal(format!("failed to reconstruct candidate passkey: {e:?}")))?;
        let candidate_state = DiscoverableAuthenticationServerState::decode(encoded_state.as_slice())
            .map_err(|e| AppError::Internal(format!("failed to decode ceremony state: {e:?}")))?;
        if candidate_state.verify(&config.rp_id, &credential, &mut candidate, &options).is_ok() {
            matched = Some((row.get("id"), row.get("user_id"), row.get("is_admin")));
            break;
        }
    }
    let (passkey_id, user_id, is_admin) = matched.ok_or(AppError::UnknownPasskey)?;

    // No per-credential state to persist on success - counter/backup flags
    // aren't tracked (see above) - just record when this passkey was last
    // used, for the user's own "my passkeys" list.
    sqlx::query("UPDATE passkeys SET last_used_at = datetime('now') WHERE id = ?")
        .bind(passkey_id)
        .execute(&state.pool)
        .await?;

    info!(
        "passkey login/finish: user_id={user_id} signed in (matched against {candidate_count} \
         candidate passkeys server-wide)"
    );
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
) -> Result<Json<StartResponse>, AppError> {
    let (ceremony_id, options) = start_registration(&state, Some(auth.0))?;
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
    AppJson(req): AppJson<FinishRequest<webauthn_rp::response::register::ser_relaxed::RegistrationRelaxed>>,
) -> Result<(StatusCode, Json<PasskeySummary>), AppError> {
    let (user_id, for_user) = finish_registration(&state, req.ceremony_id, &req.credential.0).await?;
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
