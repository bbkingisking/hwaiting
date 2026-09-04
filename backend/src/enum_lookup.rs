use sqlx::{Sqlite, Transaction};

use crate::error::AppError;

/// Process-lifetime cache of `languages.id`, per slug. `languages` is
/// seeded once by migration and never changes at runtime, so there's
/// nothing to invalidate - every caller that used to hardcode
/// `language_tag = 'en'` (before migration 20240101000045) binds
/// `eng_language_id` instead. `kor_language_id` exists alongside it only for
/// the one place a Korean label ships next to its English one on purpose
/// (`InflectionFormValue`, see field_values.rs) - everywhere else stays
/// hardcoded to `eng`, matching the app's current all-English status quo.
/// Also the one place a future real per-request language selector would
/// plug in.
static ENG_LANGUAGE_ID: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
static KOR_LANGUAGE_ID: std::sync::OnceLock<i64> = std::sync::OnceLock::new();

async fn cached_language_id<'e, E>(
    executor: E,
    cache: &'static std::sync::OnceLock<i64>,
    slug: &str,
) -> Result<i64, AppError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    if let Some(&id) = cache.get() {
        return Ok(id);
    }
    let id: i64 = sqlx::query_scalar("SELECT id FROM languages WHERE slug = ?")
        .bind(slug)
        .fetch_one(executor)
        .await?;
    // If another task raced us here, both queried the same immutable row -
    // whichever wins `get_or_init` is the same value either way.
    Ok(*cache.get_or_init(|| id))
}

pub async fn eng_language_id<'e, E>(executor: E) -> Result<i64, AppError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    cached_language_id(executor, &ENG_LANGUAGE_ID, "eng").await
}

pub async fn kor_language_id<'e, E>(executor: E) -> Result<i64, AppError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    cached_language_id(executor, &KOR_LANGUAGE_ID, "kor").await
}

/// (labels_table, fk_column) for a lookup table's `_labels` child - see
/// migration 20240101000041. Neither name is derivable from `table` by a
/// fixed rule (`parts_of_speech_labels` uses `pos_id`, not
/// `parts_of_speech_id`), so this is the one place that mapping lives;
/// `cards::field_values::fetch_field_values` reuses it rather than
/// hand-repeating it a second time.
pub(crate) fn labels_table(table: &str) -> (&'static str, &'static str) {
    match table {
        "grammar_patterns" => ("grammar_patterns_labels", "grammar_pattern_id"),
        "parts_of_speech" => ("parts_of_speech_labels", "pos_id"),
        "origin_types" => ("origin_types_labels", "origin_type_id"),
        "grades" => ("grades_labels", "grade_id"),
        "speech_levels" => ("speech_levels_labels", "speech_level_id"),
        "tenses" => ("tenses_labels", "tense_id"),
        _ => unreachable!("no _labels table registered for {table}"),
    }
}

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
