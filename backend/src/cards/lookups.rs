//! `GET /api/cards/enum-lookups` - lookup-table values (parts of speech,
//! grades, grammar patterns, ...) for populating dropdowns/badges, both in
//! the review UI and in admin/authoring surfaces.

use axum::{extract::State, Json};
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use utoipa::ToSchema;

use crate::error::AppError;

#[derive(Serialize, ToSchema)]
pub struct EnumEntry {
    pub slug: String,
    pub label: String,
    pub tooltip: Option<String>,
    pub rank: Option<i64>,
    pub endings: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct EnumLookups {
    pub pos: Vec<EnumEntry>,
    pub origin_type: Vec<EnumEntry>,
    pub grade: Vec<EnumEntry>,
    pub speech_level: Vec<EnumEntry>,
    pub tense: Vec<EnumEntry>,
    pub grammar_pattern: Vec<EnumEntry>,
}

async fn fetch_enum_entries(
    pool: &SqlitePool,
    table: &str,
    has_tooltip: bool,
    has_rank: bool,
    has_endings: bool,
) -> Result<Vec<EnumEntry>, AppError> {
    let cols = format!(
        "slug, label{}{}{}",
        if has_tooltip { ", tooltip" } else { "" },
        if has_rank { ", rank" } else { "" },
        if has_endings { ", endings" } else { "" },
    );
    let rows = sqlx::query(&format!("SELECT {cols} FROM {table} ORDER BY id"))
        .fetch_all(pool)
        .await?;

    Ok(rows
        .iter()
        .map(|row| EnumEntry {
            slug: row.get("slug"),
            label: row.get("label"),
            tooltip: if has_tooltip { row.get("tooltip") } else { None },
            rank: if has_rank { row.get("rank") } else { None },
            endings: if has_endings { row.get("endings") } else { None },
        })
        .collect())
}

#[utoipa::path(
    get,
    path = "/api/cards/enum-lookups",
    responses(
        (status = 200, description = "Lookup-table values for dropdowns/enum displays", body = EnumLookups),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn list_enum_lookups(
    State(pool): State<SqlitePool>,
    _auth: crate::auth::AuthUser,
) -> Result<Json<EnumLookups>, AppError> {
    Ok(Json(EnumLookups {
        pos: fetch_enum_entries(&pool, "parts_of_speech", false, false, false).await?,
        origin_type: fetch_enum_entries(&pool, "origin_types", false, false, false).await?,
        grade: fetch_enum_entries(&pool, "grades", false, true, false).await?,
        speech_level: fetch_enum_entries(&pool, "speech_levels", false, false, false).await?,
        tense: fetch_enum_entries(&pool, "tenses", false, false, false).await?,
        grammar_pattern: fetch_enum_entries(&pool, "grammar_patterns", true, false, true).await?,
    }))
}
