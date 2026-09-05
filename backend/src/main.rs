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
mod user;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Static OpenAPI export: `PRINT_OPENAPI=1 ./hwaiting` prints the spec to
    // stdout and exits, without touching the DB, credentials, or anything
    // else - used to feed frontend type generation from CI/local builds
    // without needing a running server.
    if env::var("PRINT_OPENAPI").is_ok() {
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

    // Serve static files from STATIC_DIR, if set. Unset means API-only mode:
    // no fallback service, unmatched paths just 404.
    match env::var("STATIC_DIR").ok().filter(|s| !s.trim().is_empty()) {
        Some(static_dir) => {
            tracing::info!("Serving static files from {}", static_dir);
            let index_path = format!("{}/index.html", static_dir);
            let serve_dir = ServeDir::new(&static_dir)
                .not_found_service(ServeFile::new(index_path));
            app = app.fallback_service(serve_dir);
        }
        None => {
            tracing::info!("STATIC_DIR not set - running in API-only mode (no static file serving)");
        }
    }

    // CORS: only add the layer if origins are explicitly configured. Unset
    // means same-origin only, enforced by the browser for free - the
    // correct default when STATIC_DIR is serving the frontend from this
    // same binary.
    match env::var("CORS_ALLOWED_ORIGINS").ok().filter(|s| !s.trim().is_empty()) {
        Some(origins) => {
            let allowed_origins: Vec<HeaderValue> = origins
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| {
                    s.parse::<HeaderValue>()
                        .unwrap_or_else(|e| panic!("Invalid origin '{}' in CORS_ALLOWED_ORIGINS: {}", s, e))
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
            tracing::info!("CORS_ALLOWED_ORIGINS not set - no CORS layer added (same-origin only)");
        }
    }

    // Read HOST and PORT from environment variables
    let host = env::var("HOST")
        .expect("HOST environment variable must be set");
    let port: u16 = env::var("PORT")
        .expect("PORT environment variable must be set")
        .parse()
        .expect("PORT must be a valid u16 number");

    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .expect("Failed to parse HOST:PORT into SocketAddr");

    tracing::info!("Backend listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
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

