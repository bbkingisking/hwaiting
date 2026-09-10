use axum::{
    http::{header, HeaderValue, Method},
    routing::{delete, get, patch, post, put},
    Router,
};
use std::net::SocketAddr;
use std::env;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[cfg(debug_assertions)]
use dotenvy::dotenv;

mod admin;
mod auth;
mod cards;
mod credentials;
mod db;
mod enum_lookup;
mod error;
mod export_import;
mod inflection_hints;
mod openapi;
mod passkey;
#[cfg(test)]
mod test_support;
mod user;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // `--help`/`-h` prints usage and the env-var config surface, then exits
    // before anything else runs - same reasoning as `--print-openapi` below:
    // a one-off "tell me how to run this" instruction belongs on argv, not
    // in credentials.rs's config surface.
    if env::args().any(|arg| arg == "--help" || arg == "-h") {
        print!("{}", help_text());
        return Ok(());
    }

    // Static OpenAPI export: `./hwaiting --print-openapi` prints the spec to
    // stdout and exits, without touching the DB, credentials, or anything
    // else - used to feed frontend type generation from CI/local builds
    // without needing a running server. A CLI flag rather than an env var:
    // this isn't a deployment setting that belongs alongside
    // credentials.rs's config surface, it's a one-off "run in a different
    // mode this one time" instruction, and argv is the channel for that.
    if env::args().any(|arg| arg == "--print-openapi") {
        println!("{}", openapi::ApiDoc::openapi().to_pretty_json()?);
        return Ok(());
    }

    // Load .env file in debug builds only
    #[cfg(debug_assertions)]
    {
        if let Err(e) = dotenv() {
            tracing::warn!("Failed to load .env file: {}", e);
        }
    }

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hwaiting=info,tower_http=info,axum=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::debug!("Starting Annyeong backend...");

    // Initialize database
    let pool = db::init().await?;

    // Router state: wraps the pool plus the WebAuthn passkey machinery
    // (RP config, in-memory ceremony table). Every handler outside
    // `passkey.rs` still declares `State<SqlitePool>` unchanged - see
    // `passkey::AppState`'s `FromRef<AppState> for SqlitePool` impl.
    let state = passkey::AppState::new(pool);

    // Build API routes
    let api_routes = Router::new()
        .route("/capabilities", get(passkey::capabilities))
        .route("/auth/login", post(auth::login))
        .route("/auth/signup", post(auth::signup))
        .route("/auth/passkey/register/start", post(passkey::register_start))
        .route("/auth/passkey/register/finish", post(passkey::register_finish))
        .route("/auth/passkey/login/start", post(passkey::login_start))
        .route("/auth/passkey/login/finish", post(passkey::login_finish))
        .route("/cards/next", get(cards::get_next_card))
        .route("/cards/field-values", get(cards::list_field_values))
        .route("/cards/{card_id}/check", post(cards::check_answer))
        .route("/cards/{card_id}/comment", post(cards::comment_on_card))
        .route("/cards/{card_id}/suppress", put(cards::suppress_card))
        .route("/cards/suppressed", get(cards::list_suppressed_cards))
        .route("/cards/hanja-drill", get(cards::get_hanja_drill))
        .route("/cards/{card_id}/unsuppress", put(cards::unsuppress_card))
        .route("/cards/stats", get(cards::get_stats))
        .route("/cards/history", get(cards::get_history))
        .route("/cards/fsrs-parameters", post(cards::optimize_fsrs))
        .route("/cards/fsrs-parameters", delete(cards::reset_fsrs_parameters))
        .route("/user/me", get(user::get_profile))
        .route("/user/settings", get(user::get_settings))
        .route("/user/settings", patch(user::update_settings))
        .route("/user/export", get(export_import::export_data))
        .route("/user/import", post(export_import::import_data))
        .route("/user/passkeys", get(passkey::list_passkeys))
        .route("/user/passkeys/register/start", post(passkey::add_passkey_start))
        .route("/user/passkeys/register/finish", post(passkey::add_passkey_finish))
        .route("/user/passkeys/{passkey_id}", delete(passkey::delete_passkey))
        .route("/admin/users", get(admin::list_users))
        .route("/admin/cards/search", get(admin::search_cards))
        .route("/admin/cards/{card_id}", patch(admin::edit_card))
        .route("/admin/cards/{card_id}/inflections", get(admin::get_card_inflections))
        .route("/health", get(health_check))
        .with_state(state);

    // Combine routes - API takes precedence over static files
    let mut app = Router::new()
        .nest("/api", api_routes)
        .merge(SwaggerUi::new("/api/docs").url("/api/openapi.json", openapi::ApiDoc::openapi()));

    // Serve static files from HWAITING_STATIC_DIR, if set. Unset means
    // API-only mode: no fallback service, unmatched paths just 404.
    match credentials::static_dir().filter(|s| !s.trim().is_empty()) {
        Some(static_dir) => {
            tracing::info!("Serving static files from {}", static_dir);
            let index_path = format!("{}/index.html", static_dir);
            let serve_dir = ServeDir::new(&static_dir)
                .not_found_service(ServeFile::new(index_path));
            app = app.fallback_service(serve_dir);
        }
        None => {
            tracing::info!("HWAITING_STATIC_DIR not set - running in API-only mode (no static file serving)");
        }
    }

    // CORS: only add the layer if origins are explicitly configured. Unset
    // means same-origin only, enforced by the browser for free - the
    // correct default when HWAITING_STATIC_DIR is serving the frontend from
    // this same binary.
    match credentials::cors_allowed_origins().filter(|s| !s.trim().is_empty()) {
        Some(origins) => {
            let allowed_origins: Vec<HeaderValue> = origins
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| {
                    s.parse::<HeaderValue>()
                        .unwrap_or_else(|e| panic!("Invalid origin '{}' in HWAITING_CORS_ALLOWED_ORIGINS: {}", s, e))
                })
                .collect();

            tracing::info!("CORS enabled for origins: {:?}", allowed_origins);

            let cors = CorsLayer::new()
                .allow_origin(AllowOrigin::list(allowed_origins))
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                ])
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

            app = app.layer(cors);
        }
        None => {
            tracing::info!("HWAITING_CORS_ALLOWED_ORIGINS not set - no CORS layer added (same-origin only)");
        }
    }

    // Bind and serve. Three ways to get a listening socket, tried in order:
    //
    //  1. systemd socket activation (LISTEN_PID/LISTEN_FDS set): the socket
    //     is already bound in the host's network namespace before this
    //     process even starts, so the unit can run fully network-isolated
    //     (PrivateNetwork=yes) and never has to call socket() itself.
    //  2. HWAITING_UNIX_SOCKET=<path>: self-bind a Unix domain socket, for
    //     setups that reverse-proxy over a local socket file without using
    //     systemd socket activation.
    //  3. HWAITING_HOST + HWAITING_PORT: the original TCP listener -
    //     unchanged, still what prod uses. Defaults to 127.0.0.1:3000 when
    //     unset, since neither value is a secret or deployment-specific in a
    //     way that makes a default unsafe - unlike HWAITING_RP_ID/HWAITING_RP_ORIGINS, which
    //     have none.
    if let Some(std_listener) = systemd_activated_unix_socket() {
        let listener = tokio::net::UnixListener::from_std(std_listener)?;
        tracing::info!("Backend listening on systemd-activated unix socket");
        axum::serve(listener, app).await?;
    } else if let Some(path) = credentials::unix_socket() {
        // Remove a stale socket file left behind by an unclean previous exit.
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path)?;
        tracing::info!("Backend listening on unix socket {}", path);
        axum::serve(listener, app).await?;
    } else {
        let host = credentials::host();
        let port: u16 = credentials::port()
            .parse()
            .expect("HWAITING_PORT must be a valid u16 number");

        let addr: SocketAddr = format!("{}:{}", host, port)
            .parse()
            .expect("Failed to parse HWAITING_HOST:HWAITING_PORT into SocketAddr");

        tracing::info!("Backend listening on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;
    }

    Ok(())
}

/// Picks up a systemd socket-activated Unix listener passed in as fd 3
/// (`SD_LISTEN_FDS_START`), if `LISTEN_PID`/`LISTEN_FDS` confirm one was
/// handed to this exact process. This is what lets a unit set
/// `PrivateNetwork=yes` and `RestrictAddressFamilies=AF_UNIX`: systemd
/// creates and binds the socket in the host's network namespace before this
/// process (and its own private network namespace) exists, so the service
/// itself never calls `socket()`.
fn systemd_activated_unix_socket() -> Option<std::os::unix::net::UnixListener> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    let listen_pid: u32 = env::var("LISTEN_PID").ok()?.parse().ok()?;
    if listen_pid != std::process::id() {
        return None;
    }
    let listen_fds: i32 = env::var("LISTEN_FDS").ok()?.parse().ok()?;
    if listen_fds < 1 {
        return None;
    }

    // SAFETY: LISTEN_PID matching our own pid confirms systemd handed fd 3
    // (SD_LISTEN_FDS_START) to this exact exec, and nothing earlier in this
    // process opens or closes low-numbered fds - so we're the sole owner.
    // OwnedFd first, rather than constructing the typed listener directly,
    // so "take ownership of an externally-handed-over fd" and "what type
    // is this" are two separate, individually narrow steps.
    let fd = unsafe { OwnedFd::from_raw_fd(3) };

    // systemd hands the fd over with FD_CLOEXEC *cleared* - it has to
    // survive the exec() into this binary, and doesn't get re-set
    // afterward. Left alone, any subprocess this process later spawns
    // would silently inherit the listening socket too. Nothing here
    // shells out today, so this isn't exploitable yet, but it's cheap
    // enough to close off regardless of whether that stays true.
    //
    // SAFETY: fcntl(F_GETFD)/F_SETFD on a valid, owned fd we're not
    // otherwise touching concurrently - both calls are just flag reads/
    // writes, no memory safety involved.
    unsafe {
        let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFD);
        if flags >= 0 {
            libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC);
        }
    }

    let listener = std::os::unix::net::UnixListener::from(fd);
    listener.set_nonblocking(true).ok()?;
    Some(listener)
}

/// Text for `--help`/`-h`. A plain function returning a `String` rather than
/// a `const &str` so it can interpolate `CARGO_PKG_VERSION` - the one bit
/// that isn't known until compile time either way, but is cleaner to splice
/// in here than to hand-copy into a literal.
///
/// The env vars listed are exactly credentials.rs's config surface (see that
/// file's doc comment on `read_config`): every one of them, in the same
/// order they're defined there, is also readable as a systemd credential
/// file named after the lowercased, dash-separated env var (e.g.
/// `HWAITING_JWT_SECRET` -> `$CREDENTIALS_DIRECTORY/hwaiting-jwt-secret`),
/// with the env var winning if both are set. That mechanism itself isn't
/// repeated per-line below - just noted once - so this list stays in sync
/// with credentials.rs by inspection rather than needing an update every
/// time someone skims past it.
fn help_text() -> String {
    format!(
        "hwaiting {version}
Korean flashcard backend (axum/SQLite), also serving the frontend SPA.

USAGE:
    hwaiting [OPTIONS]

OPTIONS:
    -h, --help          Print this help message and exit
        --print-openapi Print the OpenAPI spec as JSON to stdout and exit
                         (no DB or config access)

ENVIRONMENT VARIABLES:
    Every variable below can also be set via a systemd credential file at
    $CREDENTIALS_DIRECTORY/<name, lowercased, underscores to dashes> (e.g.
    HWAITING_JWT_SECRET -> hwaiting-jwt-secret); the env var wins if both
    are set.

    Required, no default:
        HWAITING_JWT_SECRET             Secret used to sign auth JWTs
        HWAITING_ADMIN_PASSWORD         Password for the admin account

    Optional, with a default:
        HWAITING_ADMIN_USERNAME         Admin account username (default: admin)
        HWAITING_HOST                   TCP bind host (default: 127.0.0.1)
        HWAITING_PORT                   TCP bind port (default: 3000)
        HWAITING_DATABASE_URL           sqlite:// URL
                                         (default: $XDG_DATA_HOME/hwaiting/hwaiting.db,
                                         or $HOME/.local/share/hwaiting/hwaiting.db)

    Optional, unset means the feature is off:
        HWAITING_UNIX_SOCKET            Bind a Unix socket at this path instead of TCP
        HWAITING_STATIC_DIR             Serve the frontend SPA from this directory
        HWAITING_CORS_ALLOWED_ORIGINS   Comma-separated list of allowed CORS origins
        HWAITING_RP_ID                  WebAuthn RP ID - enables passkey sign-in
        HWAITING_RP_ORIGINS             WebAuthn allowed origin(s) for passkey sign-in
        HWAITING_JWT_EXPIRY_SECONDS     Auth JWT lifetime in seconds

    Other:
        CREDENTIALS_DIRECTORY           systemd credential directory (see above)
        LISTEN_PID, LISTEN_FDS          systemd socket activation - takes priority
                                         over HWAITING_UNIX_SOCKET and
                                         HWAITING_HOST/HWAITING_PORT when both are set
                                         and LISTEN_PID matches this process
        RUST_LOG                        tracing/log filter
                                         (default: hwaiting=info,tower_http=info,axum=info)
",
        version = env!("CARGO_PKG_VERSION"),
    )
}

#[utoipa::path(
    get,
    path = "/api/health",
    responses(
        (status = 200, description = "Service is up", body = String),
    ),
    tag = "misc"
)]
async fn health_check() -> &'static str {
    "OK"
}

