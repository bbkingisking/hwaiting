use axum::{
    extract::{Path, State},
    Json,
};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};


use crate::auth::AdminUser;
use crate::error::AppError;

/// Extract a nullable string field from JSON, distinguishing absent from null.
/// Returns `Some(None)` for explicit null, `Some(Some(s))` for a string, `None` for absent.
fn get_nullable_str(obj: &serde_json::Map<String, Value>, key: &str) -> Option<Option<String>> {
    match obj.get(key) {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => Some(v.as_str().map(|s| s.to_owned())),
    }
}

/// Extract an optional string field from JSON. Returns None for absent or null.
fn get_opt_str(obj: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    match obj.get(key) {
        None | Some(Value::Null) => None,
        Some(v) => v.as_str().map(|s| s.to_owned()),
    }
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
    Json(payload): Json<Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    info!("Admin editing card {}", card_id);

    let obj = payload.as_object().ok_or(AppError::BadRequest)?;

    // Extract fields — nullable ones use get_nullable_str so we can distinguish
    // "absent" (don't touch) from "explicit null" (set to NULL).
    let word = get_opt_str(obj, "word");
    let definition = get_nullable_str(obj, "definition");
    let pos = get_nullable_str(obj, "pos");
    let origin_type = get_nullable_str(obj, "origin_type");
    let hanja = get_nullable_str(obj, "hanja");
    let hanja_eum = get_nullable_str(obj, "hanja_eum");
    let grade = get_nullable_str(obj, "grade");
    let trans_word = get_opt_str(obj, "trans_word");
    let trans_dfn = get_nullable_str(obj, "trans_dfn");
    let sentence = get_opt_str(obj, "sentence");
    let sentence_translation = get_opt_str(obj, "sentence_translation");
    let target = get_opt_str(obj, "target");
    let alternatives: Option<Vec<String>> = obj.get("alternatives").and_then(|v| {
        serde_json::from_value(v.clone()).ok()
    });
    let speech_level = get_opt_str(obj, "speech_level");
    let tense = get_opt_str(obj, "tense");

    debug!(
        "Parsed fields: word={:?}, hanja={:?}, hanja_eum={:?}, definition={:?}",
        word, hanja, hanja_eum, definition
    );

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
        if word.is_some()        { sets.push("word = ?") }
        if definition.is_some()  { sets.push("definition = ?") }
        if pos.is_some()         { sets.push("pos = ?") }
        if origin_type.is_some() { sets.push("origin_type = ?") }
        if hanja.is_some()       { sets.push("hanja = ?") }
        if hanja_eum.is_some()   { sets.push("hanja_eum = ?") }
        if grade.is_some()       { sets.push("grade = ?") }

        if !sets.is_empty() {
            let sql = format!("UPDATE cards SET {} WHERE id = ?", sets.join(", "));
            debug!("Cards update SQL: {}", sql);
            let mut q = sqlx::query(&sql);
            if let Some(ref v) = word        { q = q.bind(v.as_str()) }
            if let Some(ref v) = definition  { q = q.bind(v.as_deref()) }
            if let Some(ref v) = pos         { q = q.bind(v.as_deref()) }
            if let Some(ref v) = origin_type { q = q.bind(v.as_deref()) }
            if let Some(ref v) = hanja       { q = q.bind(v.as_deref()) }
            if let Some(ref v) = hanja_eum   { q = q.bind(v.as_deref()) }
            if let Some(ref v) = grade       { q = q.bind(v.as_deref()) }
            let result = q.bind(card_id).execute(&mut *tx).await?;
            debug!("Cards update rows_affected: {}", result.rows_affected());
        }
    }

    // Update card_translations (first English row)
    {
        let mut sets: Vec<&str> = Vec::new();
        if trans_word.is_some() { sets.push("trans_word = ?") }
        if trans_dfn.is_some()  { sets.push("trans_dfn = ?") }

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
                if let Some(ref v) = trans_word { q = q.bind(v.as_str()) }
                if let Some(ref v) = trans_dfn  { q = q.bind(v.as_deref()) }
                q.bind(card_id).execute(&mut *tx).await?;
            }
        }
    }

    // Update sentences + sentence_translations (first sentence row for this card)
    if sentence.is_some() || target.is_some() || sentence_translation.is_some() {
        let sentence_id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM sentences WHERE card_id = ? ORDER BY id LIMIT 1")
                .bind(card_id)
                .fetch_optional(&mut *tx)
                .await?;

        if let Some(sid) = sentence_id {
            let mut sets: Vec<&str> = Vec::new();
            if sentence.is_some() { sets.push("text = ?") }
            if target.is_some()   { sets.push("target = ?") }

            if !sets.is_empty() {
                let sql = format!("UPDATE sentences SET {} WHERE id = ?", sets.join(", "));
                let mut q = sqlx::query(&sql);
                if let Some(ref v) = sentence { q = q.bind(v.as_str()) }
                if let Some(ref v) = target   { q = q.bind(v.as_str()) }
                q.bind(sid).execute(&mut *tx).await?;
            }

            if let Some(ref st) = sentence_translation {
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

    // Update alternative targets
    if let Some(ref alts) = alternatives {
        let sentence_id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM sentences WHERE card_id = ? ORDER BY id LIMIT 1")
                .bind(card_id)
                .fetch_optional(&mut *tx)
                .await?;

        if let Some(sid) = sentence_id {
            // Delete existing alternatives
            sqlx::query("DELETE FROM sentence_alternative_targets WHERE sentence_id = ?")
                .bind(sid)
                .execute(&mut *tx)
                .await?;

            // Insert new alternatives
            for alt in alts {
                let trimmed = alt.trim();
                if !trimmed.is_empty() {
                    sqlx::query(
                        "INSERT INTO sentence_alternative_targets (sentence_id, alt_target) VALUES (?, ?)"
                    )
                    .bind(sid)
                    .bind(trimmed)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
    }

    // Update sentence_inflection_hints (speech_level / tense)
    if speech_level.is_some() || tense.is_some() {
        let sentence_id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM sentences WHERE card_id = ? ORDER BY id LIMIT 1")
                .bind(card_id)
                .fetch_optional(&mut *tx)
                .await?;

        if let Some(sid) = sentence_id {
            let hint_exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sentence_inflection_hints WHERE sentence_id = ?)")
                    .bind(sid)
                    .fetch_one(&mut *tx)
                    .await?;

            if hint_exists {
                if let Some(ref sl) = speech_level {
                    sqlx::query("UPDATE sentence_inflection_hints SET speech_level = ? WHERE sentence_id = ?")
                        .bind(sl.as_str())
                        .bind(sid)
                        .execute(&mut *tx)
                        .await?;
                }
                if let Some(ref t) = tense {
                    sqlx::query("UPDATE sentence_inflection_hints SET tense = ? WHERE sentence_id = ?")
                        .bind(t.as_str())
                        .bind(sid)
                        .execute(&mut *tx)
                        .await?;
                }
            } else if speech_level.is_some() || tense.is_some() {
                sqlx::query(
                    "INSERT INTO sentence_inflection_hints (sentence_id, speech_level, tense) VALUES (?, ?, ?)"
                )
                .bind(sid)
                .bind(speech_level.as_deref())
                .bind(tense.as_deref())
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