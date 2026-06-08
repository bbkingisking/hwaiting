use axum::{
    routing::{delete, get, patch, post, put},
    Router,
};
use std::net::SocketAddr;
use std::env;
use tower_http::services::{ServeDir, ServeFile};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(debug_assertions)]
use dotenvy::dotenv;

mod admin;
mod auth;
mod cards;
mod credentials;
mod custom_cards;
mod db;
mod error;
mod export_import;
mod user;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
                .unwrap_or_else(|_| "annyeong_backend=debug,tower_http=debug,axum=debug".into()),
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
        .route("/user/settings", post(user::update_settings))
        .route("/user/export", get(export_import::export_data))
        .route("/user/import", post(export_import::import_data))
        .route("/admin/invites", get(admin::list_invites))
        .route("/admin/invites", post(admin::generate_invites))
        .route("/admin/invites/{code}", delete(admin::delete_invite))
        .route("/admin/cards/{card_id}", patch(admin::edit_card))
        .route("/custom-cards", get(custom_cards::list_custom_cards))
        .route("/custom-cards", post(custom_cards::create_custom_card))
        .route("/custom-cards/{card_id}", get(custom_cards::get_custom_card))
        .route("/custom-cards/{card_id}", patch(custom_cards::update_custom_card))
        .route("/custom-cards/{card_id}", delete(custom_cards::delete_custom_card))
        .route("/health", get(health_check))
        .with_state(pool);

    // Serve static files from STATIC_DIR
    let static_dir = env::var("STATIC_DIR")
        .expect("STATIC_DIR environment variable must be set");
    let index_path = format!("{}/index.html", static_dir);

    let serve_dir = ServeDir::new(&static_dir)
        .not_found_service(ServeFile::new(index_path));

    // Combine routes - API takes precedence over static files
    let app = Router::new()
        .nest("/api", api_routes)
        .fallback_service(serve_dir);

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

async fn health_check() -> &'static str {
    "OK"
}

