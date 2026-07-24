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
mod custom_cards;
mod db;
mod enum_lookup;
mod error;
mod export_import;
mod openapi;
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
                .unwrap_or_else(|_| "hwaiting=debug,tower_http=debug,axum=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::debug!("Starting Annyeong backend...");

    // Initialize database
    let pool = db::init().await?;

    // Build API routes
    let api_routes = Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/signup", post(auth::signup))
        .route("/cards/next", get(cards::get_next_card))
        .route("/cards/enum-lookups", get(cards::list_enum_lookups))
        .route("/cards/{card_id}/review", post(cards::submit_review))
        .route("/cards/{card_id}/suppress", put(cards::suppress_card))
        .route("/cards/suppressed", get(cards::list_suppressed_cards))
        .route("/cards/{card_id}/unsuppress", put(cards::unsuppress_card))
        .route("/cards/stats", get(cards::get_stats))
        .route("/cards/history", get(cards::get_review_history))
        .route("/cards/history-summary", get(cards::get_history_summary))
        .route("/cards/history-breakdown", get(cards::get_history_breakdown))
        .route("/cards/optimize-fsrs", post(cards::optimize_fsrs))
        .route("/cards/optimize-fsrs", delete(cards::reset_fsrs_parameters))
        .route("/user/me", get(user::get_profile))
        .route("/user/settings", get(user::get_settings))
        .route("/user/settings", patch(user::update_settings))
        .route("/user/export", get(export_import::export_data))
        .route("/user/import", post(export_import::import_data))
        .route("/admin/invites", get(admin::list_invites))
        .route("/admin/invites", post(admin::generate_invites))
        .route("/admin/invites/{code}", delete(admin::delete_invite))
        .route("/admin/cards/search", get(admin::search_cards))
        .route("/admin/cards/{card_id}", patch(admin::edit_card))
        .route("/custom-cards", get(custom_cards::list_custom_cards))
        .route("/custom-cards", post(custom_cards::create_custom_card))
        .route("/custom-cards/{card_id}", get(custom_cards::get_custom_card))
        .route("/custom-cards/{card_id}", patch(custom_cards::update_custom_card))
        .route("/custom-cards/{card_id}", delete(custom_cards::delete_custom_card))
        .route("/health", get(health_check))
        .with_state(pool);

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

