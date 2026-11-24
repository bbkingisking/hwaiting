use sqlx::{Sqlite, Transaction};

use crate::error::AppError;

/// Resolve a nullable slug field to a lookup table's row id, distinguishing
/// "absent" (don't touch) from "explicit null" (clear it) from "a slug to resolve".
/// `table` must be a trusted constant (interpolated into SQL) — never user input.
pub async fn resolve_optional_id<T: std::fmt::Display>(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    slug: Option<Option<T>>,
) -> Result<Option<Option<i64>>, AppError> {
    match slug {
        None => Ok(None),
        Some(None) => Ok(Some(None)),
        Some(Some(s)) => {
            let id: i64 = sqlx::query_scalar(&format!("SELECT id FROM {table} WHERE slug = ?"))
                .bind(s.to_string())
                .fetch_one(&mut **tx)
                .await
                .map_err(|_| AppError::BadRequest(format!("Unknown {table} value: '{s}'")))?;
            Ok(Some(Some(id)))
        }
    }
}
