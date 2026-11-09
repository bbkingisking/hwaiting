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
    Json,
    extract::{FromRef, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::info;
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;
use webauthn_rp::{
    AuthenticatedCredential, DiscoverableAuthentication16, DiscoverableAuthenticationServerState,
    Registration, RegistrationServerState,
    bin::{Decode, Encode},
    request::{
        AsciiDomain, RpId,
        auth::{AuthenticationVerificationOptions, DiscoverableCredentialRequestOptions},
        register::{
            Nickname, PublicKeyCredentialCreationOptions, PublicKeyCredentialUserEntity,
            RegistrationVerificationOptions, UserHandle16, Username,
        },
    },
    response::{
        AuthenticatorAttachment,
        auth::AuthenticatorData as AssertionAuthenticatorData,
        register::{
            AuthenticatorExtensionOutputStaticState, CompressedPubKey, CredentialProtectionPolicy,
            DynamicState, StaticState, UncompressedPubKey,
        },
    },
};

use crate::auth::{AuthResponse, AuthUser, authenticated_response};
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

/// Public-key-only format: one algorithm byte (0 Ed25519, 1 P-256,
/// 2 P-384, 3 RSA), followed by key material. EC keys use SEC1 compressed
/// points (02/03 prefix plus x); RSA uses a big-endian u32 exponent then
/// a big-endian u16 modulus length and the modulus. No extension state.
fn encode_public_key(key: &UncompressedPubKey<'_>) -> Vec<u8> {
    let mut bytes = Vec::new();
    match key {
        UncompressedPubKey::Ed25519(key) => {
            bytes.push(0);
            bytes.extend_from_slice(key.inner());
        }
        UncompressedPubKey::P256(key) => {
            bytes.extend_from_slice(&[1, 2 | (key.y()[31] & 1)]);
            bytes.extend_from_slice(key.x());
        }
        UncompressedPubKey::P384(key) => {
            bytes.extend_from_slice(&[2, 2 | (key.y()[47] & 1)]);
            bytes.extend_from_slice(key.x());
        }
        UncompressedPubKey::Rsa(key) => {
            bytes.push(3);
            bytes.extend_from_slice(&key.e().to_be_bytes());
            let len =
                u16::try_from(key.n().len()).expect("library limits RSA moduli to 2048 bytes");
            bytes.extend_from_slice(&len.to_be_bytes());
            bytes.extend_from_slice(key.n());
        }
    }
    bytes
}

fn decode_public_key(bytes: &[u8]) -> Result<StoredPubKey, AppError> {
    let invalid = || AppError::Internal("invalid stored passkey public key".to_string());
    match bytes.first() {
        Some(0) if bytes.len() == 33 => {
            let key: [u8; 32] = bytes[1..].try_into().map_err(|_| invalid())?;
            Ok(StoredPubKey::Ed25519(key.into()))
        }
        Some(1) if bytes.len() == 34 && matches!(bytes[1], 2 | 3) => {
            let x: [u8; 32] = bytes[2..].try_into().map_err(|_| invalid())?;
            Ok(StoredPubKey::P256((x, bytes[1] == 3).into()))
        }
        Some(2) if bytes.len() == 50 && matches!(bytes[1], 2 | 3) => {
            let x: [u8; 48] = bytes[2..].try_into().map_err(|_| invalid())?;
            Ok(StoredPubKey::P384((x, bytes[1] == 3).into()))
        }
        Some(3) if bytes.len() >= 7 => {
            let exponent = u32::from_be_bytes(bytes[1..5].try_into().map_err(|_| invalid())?);
            let len = usize::from(u16::from_be_bytes([bytes[5], bytes[6]]));
            if bytes.len() != 7 + len {
                return Err(invalid());
            }
            let key = (bytes[7..].to_vec(), exponent)
                .try_into()
                .map_err(|_| invalid())?;
            Ok(StoredPubKey::Rsa(key))
        }
        _ => Err(invalid()),
    }
}

/// A candidate credential reconstructed from one `passkeys` row for a
/// single login attempt, `'a`-tied to the assertion it's being checked
/// against (see `login_finish`).
type LoginCredential<'a> = AuthenticatedCredential<'a, 'a, 16, StoredPubKey>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RegistrationPurpose {
    NewAccount,
    AddToAccount(i64),
}

enum Ceremony {
    /// Public signup and adding a passkey to an existing account are
    /// distinct purposes, checked against the finish endpoint and caller.
    Registration {
        purpose: RegistrationPurpose,
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
/// bumping refcounts, not copying the ceremony table.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub events: crate::events::EventHub,
    /// `None` when `HWAITING_RP_ID`/`HWAITING_RP_ORIGINS` aren't configured - passkey sign-in
    /// is an optional feature, not a required one, see `build_webauthn_config`.
    webauthn: Option<Arc<WebauthnConfig>>,
    ceremonies: Arc<Mutex<HashMap<Uuid, Ceremony>>>,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            events: crate::events::EventHub::default(),
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
    let (rp_id, rp_origins) = match (
        crate::credentials::rp_id(),
        crate::credentials::rp_origins(),
    ) {
        (None, None) => {
            info!("HWAITING_RP_ID/HWAITING_RP_ORIGINS not set - passkey sign-in disabled");
            return None;
        }
        (Some(rp_id), Some(rp_origins)) => (rp_id, rp_origins),
        (Some(_), None) => panic!(
            "HWAITING_RP_ID is set but HWAITING_RP_ORIGINS is not - passkeys need both or neither"
        ),
        (None, Some(_)) => panic!(
            "HWAITING_RP_ORIGINS is set but HWAITING_RP_ID is not - passkeys need both or neither"
        ),
    };

    let ascii_domain = AsciiDomain::try_from(rp_id.clone())
        .unwrap_or_else(|e| panic!("Invalid HWAITING_RP_ID {rp_id:?}: {e:?}"));

    let origins: Vec<String> = rp_origins
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            parse_origin(s)
                .unwrap_or_else(|e| panic!("Invalid origin '{s}' in HWAITING_RP_ORIGINS: {e}"))
        })
        .collect();
    if origins.is_empty() {
        panic!("HWAITING_RP_ORIGINS must contain at least one origin");
    }

    info!("Passkey sign-in enabled for RP ID {rp_id:?}, origins {origins:?}");
    Some(WebauthnConfig {
        rp_id: RpId::Domain(ascii_domain),
        origins,
    })
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
        return Err(format!(
            "scheme must be http or https, got {:?}",
            url.scheme()
        ));
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
    Json(Capabilities {
        passkeys_enabled: state.passkeys_enabled(),
    })
}

// ---------------------------------------------------------------- shared registration logic

/// Builds the creation options both registration ceremonies (public
/// sign-up, and an authenticated user adding a second device) share; they
/// differ only in the purpose recorded with the challenge. The user handle is
/// thrown away once this returns - it's never read back after the
/// authenticator embeds it in the credential.
///
/// `PublicKeyCredentialCreationOptions::passkey` already requires
/// resident-key/user-verification (`AuthenticatorSelectionCriteria::
/// passkey`) and requests no attestation - exactly this app's policy,
/// without needing to hand-build a custom set of options for it.
fn start_registration(
    state: &AppState,
    purpose: RegistrationPurpose,
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
        purpose,
        state: reg_state,
        expires: Instant::now() + CEREMONY_TTL,
    });
    Ok((ceremony_id, options))
}

/// Consume a registration challenge only for the endpoint and account that
/// started it. This runs before WebAuthn verification or any database write.
/// A public finish presented with an account-bound ceremony is treated as an
/// unknown ceremony; an authenticated caller using another account's (or a
/// public) ceremony is forbidden.
fn take_registration_for(
    state: &AppState,
    ceremony_id: Uuid,
    expected: RegistrationPurpose,
) -> Result<RegistrationServerState<16>, AppError> {
    let ceremony = state.take_ceremony(ceremony_id)?;
    if ceremony.expired() {
        return Err(AppError::CeremonyExpired);
    }
    let Ceremony::Registration {
        purpose,
        state: reg_state,
        ..
    } = ceremony
    else {
        return Err(AppError::CeremonyNotFound);
    };
    if purpose != expected {
        return Err(match expected {
            RegistrationPurpose::AddToAccount(_) => AppError::Forbidden,
            RegistrationPurpose::NewAccount => AppError::CeremonyNotFound,
        });
    }
    Ok(reg_state)
}

/// Verifies a registration response and persists the resulting credential.
/// `expected` names the endpoint and authenticated caller; the ceremony must
/// match before verification or writing.
/// Returns the account id and the exact passkey row that was inserted.
async fn finish_registration(
    state: &AppState,
    ceremony_id: Uuid,
    credential: &Registration,
    expected: RegistrationPurpose,
) -> Result<(i64, PasskeySummary), AppError> {
    let reg_state = take_registration_for(state, ceremony_id, expected)?;

    let config = state.webauthn()?;
    let allowed_origins = config.allowed_origins();
    let options = RegistrationVerificationOptions::<&str, &str> {
        allowed_origins: &allowed_origins,
        ..Default::default()
    };
    let registered = reg_state
        .verify(&config.rp_id, credential, &options)
        .map_err(|e| AppError::Webauthn(format!("{e:?}")))?;

    let public_key_blob = encode_public_key(&registered.static_state().credential_public_key);

    // One transaction for account creation (or reuse) and the passkey row
    // itself, so a failure partway through never leaves one without the
    // other.
    let mut tx = state.pool.begin().await?;

    let user_id = match expected {
        RegistrationPurpose::AddToAccount(uid) => uid,
        RegistrationPurpose::NewAccount => {
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
    let row = sqlx::query(
        "INSERT INTO passkeys (user_id, public_key) VALUES (?, ?) \
         RETURNING id, created_at, last_used_at",
    )
    .bind(user_id)
    .bind(public_key_blob)
    .fetch_one(&mut *tx)
    .await?;
    let passkey = PasskeySummary {
        id: row.get("id"),
        created_at: row.get("created_at"),
        last_used_at: row.get("last_used_at"),
    };

    tx.commit().await?;

    Ok((user_id, passkey))
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
    let (ceremony_id, options) = start_registration(&state, RegistrationPurpose::NewAccount)?;
    info!("passkey register/start: ceremony={ceremony_id}");
    Ok(Json(StartResponse {
        ceremony_id,
        options,
    }))
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
    AppJson(req): AppJson<
        FinishRequest<webauthn_rp::response::register::ser_relaxed::RegistrationRelaxed>,
    >,
) -> Result<(StatusCode, axum::response::Response), AppError> {
    let (user_id, _) = finish_registration(
        &state,
        req.ceremony_id,
        &req.credential.0,
        RegistrationPurpose::NewAccount,
    )
    .await?;
    info!("passkey register/finish: created user_id={user_id}");

    Ok((
        StatusCode::CREATED,
        authenticated_response(user_id, None, false)?,
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
pub async fn login_start(State(state): State<AppState>) -> Result<Json<StartResponse>, AppError> {
    let config = state.webauthn()?;
    let (auth_state, client_state) = DiscoverableCredentialRequestOptions::passkey(&config.rp_id)
        .start_ceremony()
        .map_err(|e| {
            AppError::Internal(format!("failed to start authentication ceremony: {e:?}"))
        })?;

    let options = serde_json::to_value(&client_state)
        .map_err(|e| AppError::Internal(format!("failed to serialize request options: {e}")))?;

    let ceremony_id = state.store_ceremony(Ceremony::Authentication {
        state: auth_state,
        expires: Instant::now() + CEREMONY_TTL,
    });
    info!("passkey login/start: ceremony={ceremony_id}");
    Ok(Json(StartResponse {
        ceremony_id,
        options,
    }))
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
) -> Result<axum::response::Response, AppError> {
    let ceremony = state.take_ceremony(req.ceremony_id)?;
    if ceremony.expired() {
        return Err(AppError::CeremonyExpired);
    }
    let Ceremony::Authentication {
        state: auth_state, ..
    } = ceremony
    else {
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
    let asserted_backup =
        AssertionAuthenticatorData::try_from(credential.response().authenticator_data())
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
        let static_state = StaticState {
            credential_public_key: decode_public_key(&public_key)?,
            // These are application policy, not recovered registration outputs:
            // user verification is always required and PRF is never requested.
            extensions: AuthenticatorExtensionOutputStaticState {
                cred_protect: CredentialProtectionPolicy::UserVerificationRequired,
                hmac_secret: None,
            },
        };
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
                .map_err(|e| {
                    AppError::Internal(format!("failed to reconstruct candidate passkey: {e:?}"))
                })?;
        let candidate_state = DiscoverableAuthenticationServerState::decode(
            encoded_state.as_slice(),
        )
        .map_err(|e| AppError::Internal(format!("failed to decode ceremony state: {e:?}")))?;
        if candidate_state
            .verify(&config.rp_id, &credential, &mut candidate, &options)
            .is_ok()
        {
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
    authenticated_response(user_id, None, is_admin)
}

// ---------------------------------------------------------------- authenticated: manage passkeys

#[utoipa::path(
    get,
    path = "/api/user/passkeys",
    responses(
        (status = 200, description = "The caller's registered passkeys", body = ListPasskeysResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "user"
)]
pub async fn list_passkeys(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<ListPasskeysResponse>, AppError> {
    let rows = sqlx::query(
        "SELECT id, created_at, last_used_at FROM passkeys WHERE user_id = ? ORDER BY id ASC",
    )
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
    security(("cookie_auth" = [])),
    tag = "user"
)]
pub async fn add_passkey_start(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<StartResponse>, AppError> {
    let (ceremony_id, options) =
        start_registration(&state, RegistrationPurpose::AddToAccount(auth.0))?;
    info!(
        "passkey add/start: user_id={} ceremony={ceremony_id}",
        auth.0
    );
    Ok(Json(StartResponse {
        ceremony_id,
        options,
    }))
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
    security(("cookie_auth" = [])),
    tag = "user"
)]
pub async fn add_passkey_finish(
    State(state): State<AppState>,
    auth: AuthUser,
    AppJson(req): AppJson<
        FinishRequest<webauthn_rp::response::register::ser_relaxed::RegistrationRelaxed>,
    >,
) -> Result<(StatusCode, Json<PasskeySummary>), AppError> {
    let (user_id, passkey) = finish_registration(
        &state,
        req.ceremony_id,
        &req.credential.0,
        RegistrationPurpose::AddToAccount(auth.0),
    )
    .await?;

    info!(
        "passkey add/finish: user_id={user_id} added passkey id={}",
        passkey.id
    );
    Ok((StatusCode::CREATED, Json(passkey)))
}

#[utoipa::path(
    delete,
    path = "/api/user/passkeys/{passkey_id}",
    params(("passkey_id" = i64, Path, description = "Passkey ID")),
    responses(
        (status = 204, description = "Passkey removed"),
        (status = 400, description = "That passkey is the caller's only sign-in method", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 404, description = "No such passkey on the caller's account", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "user"
)]
pub async fn delete_passkey(
    State(state): State<AppState>,
    auth: AuthUser,
    AppPath(passkey_id): AppPath<i64>,
) -> Result<StatusCode, AppError> {
    // Serialize the sign-in-method check with deletion across connections.
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;

    // A password gives the account a fallback sign-in method, so only
    // block deleting the last passkey when it's the *only* one.
    let has_password: bool =
        sqlx::query_scalar("SELECT password_hash IS NOT NULL FROM users WHERE id = ?")
            .bind(auth.0)
            .fetch_one(&mut *tx)
            .await?;
    if !has_password {
        let passkey_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE user_id = ?")
                .bind(auth.0)
                .fetch_one(&mut *tx)
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
        .execute(&mut *tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    tx.commit().await?;

    info!(
        "passkey delete: user_id={} removed passkey id={passkey_id}",
        auth.0
    );
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_key_storage_contains_only_the_algorithm_and_key() {
        use webauthn_rp::response::register::Ed25519PubKey;

        // RFC 8032, section 7.1, test 1 public key.
        let bytes = [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
            0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
            0xf7, 0x07, 0x51, 0x1a,
        ];
        let key = UncompressedPubKey::Ed25519(Ed25519PubKey::try_from(bytes.as_slice()).unwrap());
        let encoded = encode_public_key(&key);
        assert_eq!(encoded.len(), 33);
        assert_eq!(encoded[0], 0);
        assert_eq!(&encoded[1..], &bytes);
        let StoredPubKey::Ed25519(decoded) = decode_public_key(&encoded).unwrap() else {
            panic!("wrong public key algorithm");
        };
        assert_eq!(decoded.into_inner(), bytes);

        for len in 0..encoded.len() {
            assert!(decode_public_key(&encoded[..len]).is_err());
        }
        let mut unknown_algorithm = encoded.clone();
        unknown_algorithm[0] = 255;
        assert!(decode_public_key(&unknown_algorithm).is_err());

        let old_format = StaticState {
            credential_public_key: key,
            extensions: AuthenticatorExtensionOutputStaticState {
                cred_protect: CredentialProtectionPolicy::UserVerificationRequired,
                hmac_secret: None,
            },
        }
        .encode()
        .unwrap();
        assert!(decode_public_key(&old_format).is_err());
    }

    #[test]
    fn ec_and_rsa_storage_preserves_key_material_and_rejects_malformed_data() {
        use webauthn_rp::response::register::{
            RsaPubKey, UncompressedP256PubKey, UncompressedP384PubKey,
        };

        // Serialization fixtures only; curve/signature validation belongs to
        // the WebAuthn verifier. Exercise both possible EC y parities.
        for parity in [0, 1] {
            let x256 = [0x42; 32];
            let y256 = [parity; 32];
            let x384 = [0x43; 48];
            let y384 = [parity; 48];
            let keys = [
                UncompressedPubKey::P256(
                    UncompressedP256PubKey::try_from((x256.as_slice(), y256.as_slice())).unwrap(),
                ),
                UncompressedPubKey::P384(
                    UncompressedP384PubKey::try_from((x384.as_slice(), y384.as_slice())).unwrap(),
                ),
            ];
            for key in keys {
                let mut encoded = encode_public_key(&key);
                match decode_public_key(&encoded).unwrap() {
                    StoredPubKey::P256(key) => {
                        assert_eq!(key.x(), &x256);
                        assert_eq!(key.y_is_odd(), parity == 1);
                        assert_eq!(encoded.len(), 34);
                    }
                    StoredPubKey::P384(key) => {
                        assert_eq!(key.x(), &x384);
                        assert_eq!(key.y_is_odd(), parity == 1);
                        assert_eq!(encoded.len(), 50);
                    }
                    _ => panic!("wrong key algorithm"),
                }
                for len in 0..encoded.len() {
                    assert!(decode_public_key(&encoded[..len]).is_err());
                }
                encoded[1] = 4;
                assert!(decode_public_key(&encoded).is_err());
                encoded[1] = 2;
                encoded.push(0);
                assert!(decode_public_key(&encoded).is_err());
            }
        }

        let modulus = [0xff; 256];
        let key =
            UncompressedPubKey::Rsa(RsaPubKey::try_from((modulus.as_slice(), 65537)).unwrap());
        let mut encoded = encode_public_key(&key);
        let StoredPubKey::Rsa(decoded) = decode_public_key(&encoded).unwrap() else {
            panic!("wrong key algorithm");
        };
        assert_eq!(decoded.n(), &modulus);
        assert_eq!(decoded.e(), 65537);
        for len in 0..encoded.len() {
            assert!(decode_public_key(&encoded[..len]).is_err());
        }
        encoded.push(0);
        assert!(decode_public_key(&encoded).is_err());
    }

    #[tokio::test]
    async fn registration_ceremonies_are_bound_to_their_endpoint_and_account() {
        let pool = crate::test_support::test_pool().await;
        let owner = crate::test_support::test_user(&pool).await;
        let other = crate::test_support::test_user(&pool).await;
        let state = AppState {
            events: crate::events::EventHub::default(),
            pool: pool.clone(),
            webauthn: Some(Arc::new(WebauthnConfig {
                rp_id: RpId::Domain(AsciiDomain::try_from("localhost".to_string()).unwrap()),
                origins: vec!["http://localhost".to_string()],
            })),
            ceremonies: Arc::new(Mutex::new(HashMap::new())),
        };
        let before_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();

        let add = add_passkey_start(State(state.clone()), AuthUser(owner))
            .await
            .unwrap()
            .0;
        assert!(matches!(
            take_registration_for(
                &state,
                add.ceremony_id,
                RegistrationPurpose::AddToAccount(other)
            ),
            Err(AppError::Forbidden)
        ));
        assert!(matches!(
            take_registration_for(
                &state,
                add.ceremony_id,
                RegistrationPurpose::AddToAccount(owner)
            ),
            Err(AppError::CeremonyNotFound)
        ));

        let add = add_passkey_start(State(state.clone()), AuthUser(owner))
            .await
            .unwrap()
            .0;
        assert!(matches!(
            take_registration_for(&state, add.ceremony_id, RegistrationPurpose::NewAccount),
            Err(AppError::CeremonyNotFound)
        ));

        let public = register_start(State(state.clone())).await.unwrap().0;
        assert_eq!(
            public.options["authenticatorSelection"]["userVerification"],
            "required"
        );
        assert!(public.options["extensions"]["prf"].is_null());
        assert!(matches!(
            take_registration_for(
                &state,
                public.ceremony_id,
                RegistrationPurpose::AddToAccount(owner)
            ),
            Err(AppError::Forbidden)
        ));

        let add = add_passkey_start(State(state.clone()), AuthUser(owner))
            .await
            .unwrap()
            .0;
        assert!(
            take_registration_for(
                &state,
                add.ceremony_id,
                RegistrationPurpose::AddToAccount(owner)
            )
            .is_ok()
        );
        let public = register_start(State(state.clone())).await.unwrap().0;
        assert!(
            take_registration_for(&state, public.ceremony_id, RegistrationPurpose::NewAccount)
                .is_ok()
        );

        // These are real, generated WebAuthn challenges. A forbidden finish
        // is rejected at this gate, before credential verification or SQL.
        let after_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        let passkeys: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM passkeys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(after_users, before_users);
        assert_eq!(passkeys, 0);
    }

    #[tokio::test]
    async fn concurrent_deletions_preserve_a_passwordless_sign_in_method() {
        let pool = crate::test_support::test_pool().await;
        let user_id = crate::test_support::test_user(&pool).await;
        let ids: Vec<i64> = sqlx::query_scalar(
            "INSERT INTO passkeys (user_id, public_key) VALUES (?, X'01'), (?, X'02') RETURNING id",
        )
        .bind(user_id)
        .bind(user_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        let state = AppState {
            events: crate::events::EventHub::default(),
            pool: pool.clone(),
            webauthn: None,
            ceremonies: Arc::new(Mutex::new(HashMap::new())),
        };

        let (first, second) = tokio::join!(
            delete_passkey(State(state.clone()), AuthUser(user_id), AppPath(ids[0])),
            delete_passkey(State(state), AuthUser(user_id), AppPath(ids[1])),
        );
        let results = [first, second];
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Ok(StatusCode::NO_CONTENT)))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(AppError::BadRequest(_))))
                .count(),
            1
        );
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 1);
    }

    #[test]
    fn accepts_plain_https_origin() {
        assert_eq!(
            parse_origin("https://hwaiting.example.com").unwrap(),
            "https://hwaiting.example.com"
        );
    }

    #[test]
    fn accepts_http_origin() {
        assert_eq!(
            parse_origin("http://localhost").unwrap(),
            "http://localhost"
        );
    }

    #[test]
    fn accepts_explicit_port() {
        assert_eq!(
            parse_origin("http://localhost:5173").unwrap(),
            "http://localhost:5173"
        );
    }

    #[test]
    fn strips_trailing_slash() {
        assert_eq!(
            parse_origin("https://hwaiting.example.com/").unwrap(),
            "https://hwaiting.example.com"
        );
    }

    #[test]
    fn rejects_other_schemes() {
        assert!(parse_origin("ftp://hwaiting.example.com").is_err());
    }

    #[test]
    fn rejects_a_path() {
        assert!(parse_origin("https://hwaiting.example.com/login").is_err());
    }

    #[test]
    fn rejects_a_query_string() {
        assert!(parse_origin("https://hwaiting.example.com?x=1").is_err());
    }

    #[test]
    fn rejects_a_fragment() {
        assert!(parse_origin("https://hwaiting.example.com#top").is_err());
    }

    #[test]
    fn rejects_userinfo() {
        assert!(parse_origin("https://user:pass@hwaiting.example.com").is_err());
    }

    // No separate "bare hostname, no scheme" test: `Url::parse` fails on a
    // schemeless string the same way it fails on any other unparseable
    // input, so this would hit the exact same early return as
    // rejects_unparseable_input below rather than the scheme-check branch.

    #[test]
    fn rejects_unparseable_input() {
        assert!(parse_origin("not a url at all").is_err());
    }

    #[test]
    fn does_not_lowercase_the_host() {
        // Documents current behavior: no case-normalization happens, even
        // though a real browser always sends a lowercase origin - an
        // operator who configures HWAITING_RP_ORIGINS with any uppercase
        // letters in the host will get an origin that a real browser
        // request can never match by exact string equality (see
        // WebauthnConfig's doc comment on why comparison is a plain string
        // compare).
        assert_eq!(
            parse_origin("https://HWaiting.Example.com").unwrap(),
            "https://HWaiting.Example.com"
        );
    }
}
