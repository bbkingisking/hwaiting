//! WebAuthn passkey sign-up/sign-in, alongside (not instead of) the
//! username/password auth in `auth.rs`. To the database and to the rest of
//! the backend this is just a second way to end up with a JWT: it stores a
//! table of credentials and otherwise defers entirely to
//! `auth::generate_token`.
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
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::{info, warn};
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;
use webauthn_rs_core::{
    internals::AuthenticatorData,
    proto::{
        Authentication, AttestationConveyancePreference, AttestationFormat, AuthenticationState,
        COSEAlgorithm, COSEKey, CreationChallengeResponse, CredProtect, Credential, CredentialID,
        CredentialProtectionPolicy, ParsedAttestation, PublicKeyCredential,
        RegisterPublicKeyCredential, RegisteredExtensions, RegistrationState,
        RequestAuthenticationExtensions, RequestChallengeResponse,
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
    /// `None` when `HWAITING_RP_ID`/`HWAITING_RP_ORIGINS` aren't configured - passkey sign-in
    /// is an optional feature, not a required one, see `build_webauthn`.
    webauthn: Option<Arc<WebauthnCore>>,
    ceremonies: Arc<Mutex<HashMap<Uuid, Ceremony>>>,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            webauthn: build_webauthn().map(Arc::new),
            ceremonies: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// The configured WebAuthn instance, or `AppError::PasskeysDisabled` if
    /// this deployment never set `HWAITING_RP_ID`/`HWAITING_RP_ORIGINS`. Every handler that
    /// needs to run an actual ceremony goes through this instead of
    /// touching the field directly.
    fn webauthn(&self) -> Result<&WebauthnCore, AppError> {
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
/// deployment isn't using it: `AppState::new` stores no `WebauthnCore`, and
/// every passkey endpoint returns `AppError::PasskeysDisabled` instead of
/// running a ceremony. Setting only one of the two, or an unparseable
/// `HWAITING_RP_ORIGINS`, is a config mistake rather than "half enabled" and still
/// panics at startup, same as before.
///
/// There's still no default derived from `HWAITING_HOST`/`HWAITING_PORT` when both are unset:
/// WebAuthn only runs in a secure context, and in production `HWAITING_RP_ID`/
/// `HWAITING_RP_ORIGINS` must be the app's real HTTPS hostname and origin, not the
/// bare `HWAITING_HOST`/`HWAITING_PORT` this binary listens on internally behind a
/// TLS-terminating proxy - so guessing from those would be wrong exactly
/// when it matters most, and silently so.
fn build_webauthn() -> Option<WebauthnCore> {
    let (rp_id, rp_origins) = match (crate::credentials::rp_id(), crate::credentials::rp_origins()) {
        (None, None) => {
            info!("HWAITING_RP_ID/HWAITING_RP_ORIGINS not set - passkey sign-in disabled");
            return None;
        }
        (Some(rp_id), Some(rp_origins)) => (rp_id, rp_origins),
        (Some(_), None) => panic!("HWAITING_RP_ID is set but HWAITING_RP_ORIGINS is not - passkeys need both or neither"),
        (None, Some(_)) => panic!("HWAITING_RP_ORIGINS is set but HWAITING_RP_ID is not - passkeys need both or neither"),
    };

    let origins: Vec<Url> = rp_origins
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            Url::parse(s).unwrap_or_else(|e| {
                panic!("Invalid origin '{s}' in HWAITING_RP_ORIGINS: {e}")
            })
        })
        .collect();
    if origins.is_empty() {
        panic!("HWAITING_RP_ORIGINS must contain at least one origin");
    }

    info!("Passkey sign-in enabled for RP ID {rp_id:?}, origins {origins:?}");
    Some(WebauthnCore::new_unsafe_experts_only(
        "hwaiting",
        &rp_id,
        origins,
        CEREMONY_TTL,
        Some(false),
        Some(false),
    ))
}

/// Rebuilds the full `webauthn-rs` `Credential` that `authenticate_credential`
/// expects, from the one field this app actually persists (`public_key`) -
/// see `20260909000000_drop_passkey_credential_id.sql` for why the rest of
/// the struct, `cred_id` included, isn't stored. `cred_id` is filled in by
/// the caller with the current assertion's own `raw_id` rather than a
/// stored value - see `login_finish`, the only caller, for why forging it
/// this way is safe and what it buys (every candidate reaches a real
/// signature check instead of being filtered out by an id that was never
/// recorded). The remaining defaults aren't guesses at "what a fresh
/// credential looks like": they're the values that make each field a no-op
/// given this app's fixed ceremony policy, so this reconstructs exactly the
/// behaviour storing the whole struct would have had here.
///
/// - `registration_policy: Required` / `user_verified: true` - both
///   `start_registration` and `login_start` hardcode
///   `UserVerificationPolicy::Required` regardless of what's stored, so
///   these never influenced the outcome even when persisted.
/// - `attestation: ParsedAttestation::default()` / `attestation_format:
///   AttestationFormat::None` - always empty anyway, since registration
///   requests `AttestationConveyancePreference::None`.
/// - `transports: None` / `extensions: RegisteredExtensions::none()` -
///   written by webauthn-rs but never read back by anything in this app.
/// - `counter: 0` / `backup_state: false` - this app keeps no per-login
///   state for anti-clone or sync-status tracking, so every login is
///   verified as if it were the credential's first.
///
/// `backup_eligible` is the one field callers must supply rather than get
/// defaulted: `verify_credential_internal`'s anti-tampering checks compare
/// it against the *current* assertion's own backup-eligible flag, and this
/// app doesn't persist a prior value to compare against (see the session
/// this was written in - reintroducing that column was considered and
/// deliberately declined). Passing the assertion's own flag back in makes
/// that comparison self-referential (always equal, so both checks - the
/// mismatch check and the unconditional backup-state-implies-eligible
/// check - degrade into asking "is this one assertion internally
/// consistent", which every spec-compliant authenticator already is, and
/// which an attacker can't forge without also forging a valid signature
/// over it) instead of comparing against a value that would otherwise be
/// permanently wrong for any backup-eligible authenticator.
fn rebuild_credential(
    cred_id: CredentialID,
    public_key: &str,
    backup_eligible: bool,
) -> Result<Credential, AppError> {
    let cred: COSEKey = serde_json::from_str(public_key).map_err(|e| {
        AppError::Internal(format!("failed to deserialize stored passkey public key: {e}"))
    })?;
    Ok(Credential {
        cred_id,
        cred,
        counter: 0,
        transports: None,
        user_verified: true,
        backup_eligible,
        backup_state: false,
        registration_policy: UserVerificationPolicy::Required,
        extensions: RegisteredExtensions::none(),
        attestation: ParsedAttestation::default(),
        attestation_format: AttestationFormat::None,
    })
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
/// differ only in what `for_user` gets recorded as. `handle` is thrown away
/// once this returns - it's never read back after the authenticator embeds
/// it in the credential.
///
/// Always registers with an empty exclude-list: excluding a device already
/// registered to this account would need its credential id on hand, and
/// this app doesn't persist one (see
/// `20260909000000_drop_passkey_credential_id.sql`). So the same physical
/// authenticator *can* end up registered to one account twice, as two
/// separate rows sharing a public key - a deliberate consequence of storing
/// nothing to exclude by, not a bug.
fn start_registration(
    state: &AppState,
    handle: Uuid,
    for_user: Option<i64>,
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

    let webauthn = state.webauthn()?;
    let builder = webauthn
        .new_challenge_register_builder(handle.as_bytes(), USER_LABEL, USER_LABEL)?
        .attestation(AttestationConveyancePreference::None)
        .credential_algorithms(COSEAlgorithm::secure_algs())
        // Mandatory: login/start sends an empty allow-list, so a
        // non-discoverable credential would never be found.
        .require_resident_key(true)
        .authenticator_attachment(None)
        .user_verification_policy(UserVerificationPolicy::Required)
        .reject_synchronised_authenticators(false)
        .exclude_credentials(None)
        .hints(None)
        .extensions(Some(extensions));
    let (options, reg_state) = webauthn.generate_challenge_register(builder)?;

    let ceremony_id = state.store_ceremony(Ceremony::Registration {
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

    let cred = state.webauthn()?.register_credential(credential, reg_state, None)?;

    // One transaction for account creation (or reuse) and the passkey row
    // itself, so a failure partway through never leaves one without the
    // other.
    let mut tx = state.pool.begin().await?;

    let user_id = match &ceremony {
        Ceremony::Registration { for_user: Some(uid), .. } => *uid,
        Ceremony::Registration { for_user: None, .. } => {
            let result = sqlx::query("INSERT INTO users DEFAULT VALUES")
                .execute(&mut *tx)
                .await?;
            result.last_insert_rowid()
        }
        Ceremony::Authentication { .. } => unreachable!("take_ceremony returned the wrong variant"),
    };

    // `cred.cred_id` - the id this authenticator just generated for itself
    // during registration - is deliberately discarded, not persisted: see
    // `20260909000000_drop_passkey_credential_id.sql`. Only the public key
    // survives; `login_finish` forges a `cred_id` per login attempt instead
    // of ever storing this one.
    let public_key_json = serde_json::to_string(&cred.cred)
        .map_err(|e| AppError::Internal(format!("failed to serialize passkey public key: {e}")))?;
    sqlx::query("INSERT INTO passkeys (user_id, public_key) VALUES (?, ?)")
        .bind(user_id)
        .bind(public_key_json)
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
    let (ceremony_id, options) = start_registration(&state, handle, None)?;
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
    let webauthn = state.webauthn()?;
    let builder = webauthn
        .new_challenge_authenticate_builder(Vec::new(), Some(UserVerificationPolicy::Required))?
        .extensions(Some(RequestAuthenticationExtensions {
            appid: None,
            uvm: Some(true),
            hmac_get_secret: None,
        }))
        // Moot either way: `login_finish` reconstructs every candidate's
        // `backup_eligible` from this same assertion (see its comments), so
        // the mismatch this flag governs never occurs. Left `false` since
        // there's nothing for it to permit.
        .allow_backup_eligible_upgrade(false)
        .hints(None);
    let (options, auth_state) = webauthn.generate_challenge_authenticate(builder)?;

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

    // This app doesn't persist a per-credential `backup_eligible` flag (see
    // `rebuild_credential`), so every candidate below is reconstructed with
    // *this assertion's own* backup-eligible flag rather than a stored one -
    // that's what makes the crate's backup-eligibility checks a no-op for
    // any spec-compliant authenticator instead of a permanent rejection of
    // every synced/platform passkey. Parsed with the crate's own parser, not
    // hand-rolled, so it can't drift from what `authenticate_credential`
    // itself computes from the same bytes.
    let asserted_backup_eligible = AuthenticatorData::<Authentication>::try_from(
        req.credential.response.authenticator_data.as_slice(),
    )
    .map_err(|_| AppError::BadRequest("malformed authenticatorData".to_string()))?
    .backup_eligible;

    // No `credential_id` is stored (see
    // `20260909000000_drop_passkey_credential_id.sql`), so there is nothing
    // to narrow this query - or the loop below - by: every passkey on the
    // server, for every account, is a candidate. Each one is handed to
    // `authenticate_credential` with its `cred_id` forged to equal this
    // assertion's own `raw_id` (see `rebuild_credential`), which makes the
    // crate's internal id-match step trivially pass every candidate through
    // to a real signature verification against its stored public key. So
    // this genuinely runs up to N ECDSA verifies per login attempt, not the
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

    let forged_cred_id = CredentialID::from(req.credential.get_credential_id().to_vec());

    let mut matched: Option<(i64, i64, bool)> = None;
    for row in &rows {
        let public_key: String = row.get("public_key");
        let candidate = rebuild_credential(forged_cred_id.clone(), &public_key, asserted_backup_eligible)?;
        auth_state.set_allowed_credentials(vec![candidate]);
        if state.webauthn()?.authenticate_credential(&req.credential, &auth_state).is_ok() {
            matched = Some((row.get("id"), row.get("user_id"), row.get("is_admin")));
            break;
        }
    }
    let (passkey_id, user_id, is_admin) = matched.ok_or(AppError::UnknownPasskey)?;

    // No per-credential state to persist on success - counter/backup flags
    // aren't tracked (see rebuild_credential) - just record when this
    // passkey was last used, for the user's own "my passkeys" list.
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
) -> Result<Json<StartResponse<CreationChallengeResponse>>, AppError> {
    // No persisted per-account handle to reuse (see module docs) - a fresh
    // one is generated per ceremony purely to satisfy the protocol's
    // requirement for a `user.id` value; nothing here ever reads it back.
    let handle = Uuid::new_v4();

    let (ceremony_id, options) = start_registration(&state, handle, Some(auth.0))?;
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

