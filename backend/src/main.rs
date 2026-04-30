use axum::{
    routing::{delete, get, post, put},
    Router,
};
use std::net::SocketAddr;
use tower_http::services::{ServeDir, ServeFile};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod admin;
mod auth;
mod cards;
mod custom_cards;
mod db;
mod error;
mod user;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "annyeong_backend=debug,tower_http=debug,axum=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Annyeong backend...");

    // Initialize database
    let pool = db::init().await?;

    // Build API routes
    let api_routes = Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/signup", post(auth::signup))
        .route("/cards/next", get(cards::get_next_card))
        .route("/cards/{word_id}/review", post(cards::submit_review))
        .route("/cards/{word_id}/suppress", put(cards::suppress_card))
        .route("/cards/suppressed", get(cards::list_suppressed_cards))
        .route("/cards/{word_id}/unsuppress", put(cards::unsuppress_card))
        .route("/cards/stats", get(cards::get_stats))
        .route("/user/me", get(user::get_profile))
        .route("/user/language", post(user::set_language))
        .route("/user/settings", get(user::get_settings))
        .route("/user/settings", post(user::update_settings))
        .route("/user/export", get(user::export_data))
        .route("/user/import", post(user::import_data))
        .route("/languages", get(user::get_languages))
        .route("/admin/invites", get(admin::list_invites))
        .route("/admin/invites", post(admin::generate_invites))
        .route("/admin/invites/{code}", delete(admin::delete_invite))
        .route("/custom-cards", get(custom_cards::list_custom_cards))
        .route("/custom-cards", post(custom_cards::create_custom_card))
        .route("/custom-cards/{card_id}", get(custom_cards::get_custom_card))
        .route("/custom-cards/{card_id}", post(custom_cards::update_custom_card))
        .route("/custom-cards/{card_id}", delete(custom_cards::delete_custom_card))
        .route("/health", get(health_check))
        .with_state(pool);

    // Serve static files from ../dist
    let serve_dir = ServeDir::new("../dist")
        .not_found_service(ServeFile::new("../dist/index.html"));

    // Combine routes - API takes precedence over static files
    let app = Router::new()
        .nest("/api", api_routes)
        .fallback_service(serve_dir);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("Backend listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> &'static str {
    "OK"
}