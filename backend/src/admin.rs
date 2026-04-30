use axum::{
    extract::{Path, State},
    Json,
};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
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

#[derive(Serialize)]
pub struct InviteCode {
    pub code: String,
    pub created_at: String,
    pub used_at: Option<String>,
    pub used_by_username: Option<String>,
}

#[derive(Serialize)]
pub struct ListInvitesResponse {
    pub codes: Vec<InviteCode>,
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

pub async fn list_invites(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
) -> Result<Json<ListInvitesResponse>, AppError> {
    info!("Listing all invite codes");
    
    let rows = sqlx::query(
        "SELECT 
            ic.code, 
            ic.created_at, 
            ic.used_at,
            u.username as used_by_username
         FROM invite_codes ic
         LEFT JOIN users u ON ic.used_by_user_id = u.id
         ORDER BY ic.created_at DESC"
    )
    .fetch_all(&pool)
    .await?;
    
    let codes = rows.into_iter().map(|row| {
        InviteCode {
            code: row.get("code"),
            created_at: row.get("created_at"),
            used_at: row.get("used_at"),
            used_by_username: row.get("used_by_username"),
        }
    }).collect();
    
    Ok(Json(ListInvitesResponse { codes }))
}

pub async fn delete_invite(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    Path(code): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    info!("Deleting invite code: {}", code);
    
    let result = sqlx::query("DELETE FROM invite_codes WHERE code = ?")
        .bind(&code)
        .execute(&pool)
        .await?;
    
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    
    info!("Invite code deleted: {}", code);
    
    Ok(Json(serde_json::json!({ "success": true })))
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