use axum::{
    extract::State,
    Json,
};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::info;

use crate::auth::AdminUser;
use crate::error::AppError;

#[derive(Deserialize)]
pub struct GenerateInvitesRequest {
    pub count: usize,
}

#[derive(Serialize)]
pub struct GeneratedInvite {
    pub code: String,
}

#[derive(Serialize)]
pub struct GenerateInvitesResponse {
    pub codes: Vec<GeneratedInvite>,
}

pub async fn generate_invites(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    Json(payload): Json<GenerateInvitesRequest>,
) -> Result<Json<GenerateInvitesResponse>, AppError> {
    let count = payload.count.min(100); // Cap at 100 codes per request
    
    info!("Generating {} invite codes", count);
    
    let mut codes = Vec::new();
    
    for _ in 0..count {
        let code = generate_code();
        
        sqlx::query("INSERT INTO invite_codes (code) VALUES (?)")
            .bind(&code)
            .execute(&pool)
            .await?;
        
        codes.push(GeneratedInvite { code });
    }
    
    info!("Successfully generated {} invite codes", codes.len());
    
    Ok(Json(GenerateInvitesResponse { codes }))
}

fn generate_code() -> String {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::rng();
    
    (0..8)
        .map(|_| {
            let idx = rng.random_range(0..CHARS.len());
            CHARS[idx] as char
        })
        .collect()
}