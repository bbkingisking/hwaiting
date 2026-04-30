use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;

mod auth;
mod db;
mod error;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize database
    let pool = db::init().await?;

    // Build API routes
    let api_routes = Router::new()
        .route("/auth/login", post(auth::login))
        .route("/health", get(health_check))
        .with_state(pool);

    // Combine routes
    let app = Router::new().nest("/api", api_routes);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Backend listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> &'static str {
    "OK"
}