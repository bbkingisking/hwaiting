use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use tower_http::services::{ServeDir, ServeFile};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod admin;
mod auth;
mod cards;
mod db;
mod error;

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
        .route("/admin/generate-invites", post(admin::generate_invites))
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