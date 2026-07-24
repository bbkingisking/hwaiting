use sqlx::{Row, Sqlite, Transaction};

use crate::error::AppError;

/// Resolve a nullable slug field to a lookup table's row id, distinguishing
/// "absent" (don't touch) from "explicit null" (clear it) from "a slug to resolve".
/// `table` must be a trusted constant (interpolated into SQL) — never user input.
pub async fn resolve_optional_id(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    slug: Option<Option<String>>,
) -> Result<Option<Option<i64>>, AppError> {
    match slug {
        None => Ok(None),
        Some(None) => Ok(Some(None)),
        Some(Some(s)) => {
            let id: i64 = sqlx::query_scalar(&format!("SELECT id FROM {table} WHERE slug = ?"))
                .bind(&s)
                .fetch_one(&mut **tx)
                .await
                .map_err(|_| AppError::BadRequest(format!("Unknown {table} value: '{s}'")))?;
            Ok(Some(Some(id)))
        }
    }
}

/// Resolve a slug to a lookup table's row id, auto-registering it (slug used as
/// both slug and label) if it doesn't already exist. Used on data import, where a
/// stale/foreign enum value should never cause the import to fail outright.
pub async fn resolve_or_create_id(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    slug: Option<String>,
) -> Result<Option<i64>, AppError> {
    let Some(s) = slug else { return Ok(None) };

    // `grades` has an extra NOT NULL `rank` column; give auto-registered grades a
    // low-priority rank of 99 rather than special-casing every other table.
    if table == "grades" {
        sqlx::query("INSERT OR IGNORE INTO grades (slug, label, rank) VALUES (?, ?, 99)")
            .bind(&s)
            .bind(&s)
            .execute(&mut **tx)
            .await?;
    } else {
        sqlx::query(&format!("INSERT OR IGNORE INTO {table} (slug, label) VALUES (?, ?)"))
            .bind(&s)
            .bind(&s)
            .execute(&mut **tx)
            .await?;
    }

    let row = sqlx::query(&format!("SELECT id FROM {table} WHERE slug = ?"))
        .bind(&s)
        .fetch_one(&mut **tx)
        .await?;

    Ok(Some(row.get("id")))
}
