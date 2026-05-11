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
pub struct EditCardRequest {
    pub word: Option<String>,
    pub definition: Option<Option<String>>,
    pub pos: Option<Option<String>>,
    pub origin_type: Option<Option<String>>,
    pub hanja: Option<Option<String>>,
    pub hanja_eum: Option<Option<String>>,
    pub grade: Option<Option<String>>,
    pub trans_word: Option<String>,
    pub trans_dfn: Option<Option<String>>,
    pub sentence: Option<String>,
    pub sentence_translation: Option<String>,
    pub target: Option<String>,
}

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

pub async fn edit_card(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    Path(card_id): Path<i64>,
    Json(payload): Json<EditCardRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    info!("Admin editing card {}", card_id);

    // Verify the card exists
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cards WHERE id = ?)")
        .bind(card_id)
        .fetch_one(&pool)
        .await?;

    if !exists {
        return Err(AppError::NotFound);
    }

    let mut tx = pool.begin().await?;

    // Update cards table — build SET clause dynamically so absent fields are untouched
    // and nullable fields can be explicitly set to NULL
    {
        let mut sets: Vec<&str> = Vec::new();
        if payload.word.is_some()        { sets.push("word = ?") }
        if payload.definition.is_some()  { sets.push("definition = ?") }
        if payload.pos.is_some()         { sets.push("pos = ?") }
        if payload.origin_type.is_some() { sets.push("origin_type = ?") }
        if payload.hanja.is_some()       { sets.push("hanja = ?") }
        if payload.hanja_eum.is_some()   { sets.push("hanja_eum = ?") }
        if payload.grade.is_some()       { sets.push("grade = ?") }

        if !sets.is_empty() {
            let sql = format!("UPDATE cards SET {} WHERE id = ?", sets.join(", "));
            let mut q = sqlx::query(&sql);
            if let Some(ref v) = payload.word        { q = q.bind(v.as_str()) }
            if let Some(ref v) = payload.definition  { q = q.bind(v.as_deref()) }
            if let Some(ref v) = payload.pos         { q = q.bind(v.as_deref()) }
            if let Some(ref v) = payload.origin_type { q = q.bind(v.as_deref()) }
            if let Some(ref v) = payload.hanja       { q = q.bind(v.as_deref()) }
            if let Some(ref v) = payload.hanja_eum   { q = q.bind(v.as_deref()) }
            if let Some(ref v) = payload.grade       { q = q.bind(v.as_deref()) }
            q.bind(card_id).execute(&mut *tx).await?;
        }
    }

    // Update card_translations (first English row)
    {
        let mut sets: Vec<&str> = Vec::new();
        if payload.trans_word.is_some() { sets.push("trans_word = ?") }
        if payload.trans_dfn.is_some()  { sets.push("trans_dfn = ?") }

        if !sets.is_empty() {
            let ct_exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM card_translations WHERE card_id = ? AND language_tag = 'en')"
            )
            .bind(card_id)
            .fetch_one(&mut *tx)
            .await?;

            if ct_exists {
                let sql = format!(
                    "UPDATE card_translations SET {} WHERE card_id = ? AND language_tag = 'en'",
                    sets.join(", ")
                );
                let mut q = sqlx::query(&sql);
                if let Some(ref v) = payload.trans_word { q = q.bind(v.as_str()) }
                if let Some(ref v) = payload.trans_dfn  { q = q.bind(v.as_deref()) }
                q.bind(card_id).execute(&mut *tx).await?;
            }
        }
    }

    // Update sentences + sentence_translations (first sentence row for this card)
    if payload.sentence.is_some() || payload.target.is_some() || payload.sentence_translation.is_some() {
        let sentence_id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM sentences WHERE card_id = ? ORDER BY id LIMIT 1")
                .bind(card_id)
                .fetch_optional(&mut *tx)
                .await?;

        if let Some(sid) = sentence_id {
            let mut sets: Vec<&str> = Vec::new();
            if payload.sentence.is_some() { sets.push("text = ?") }
            if payload.target.is_some()   { sets.push("target = ?") }

            if !sets.is_empty() {
                let sql = format!("UPDATE sentences SET {} WHERE id = ?", sets.join(", "));
                let mut q = sqlx::query(&sql);
                if let Some(ref v) = payload.sentence { q = q.bind(v.as_str()) }
                if let Some(ref v) = payload.target   { q = q.bind(v.as_str()) }
                q.bind(sid).execute(&mut *tx).await?;
            }

            if let Some(ref st) = payload.sentence_translation {
                sqlx::query(
                    "UPDATE sentence_translations SET translation = ? WHERE sentence_id = ?",
                )
                .bind(st.as_str())
                .bind(sid)
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    tx.commit().await?;

    info!("Card {} updated successfully", card_id);
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