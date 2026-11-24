//! The shape of a target's tagged grammatical facts, shared across every
//! surface that reads or writes the `targets` table (see migration
//! 20240101000026 - `speech_level_id`/`tense_id`/`is_honorific`/
//! `is_humble` all live on that table, alongside the target's own `form`).
//! Previously hand-declared per-struct at each of `cards::CardFront` and
//! export_import's now-deleted `InflectionHintExport` (and independently
//! again as request fields on the create/update side) -- the exact "same
//! shape, agreeing on every field until one drifts" risk this module exists
//! to avoid. `is_honorific`/`is_humble` (added by
//! migration 20240101000025) are the two fields most likely to repeat that
//! history if left to be added separately at each call site, so they're
//! introduced here once instead.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Grammatical hints for the sentence target. Slugs identify its speech level
/// and tense; subject and object honorification are independent flags.
#[derive(Serialize, Deserialize, Clone, Default, ToSchema)]
pub struct InflectionHint {
    #[schema(required)]
    pub speech_level: Option<String>,
    #[schema(required)]
    pub tense: Option<String>,
    /// Subject honorification (주체높임, -(으)시-).
    #[serde(default)]
    #[schema(required)]
    pub is_honorific: bool,
    /// Object honorification (객체높임/겸양법: 드리다, 뵙다, 여쭙다, 모시다).
    #[serde(default)]
    #[schema(required)]
    pub is_humble: bool,
}

impl InflectionHint {
    /// Reads the four columns by name from a row whose query joined
    /// `targets` (aliasing `speech_levels.slug`/`tenses.slug` as
    /// `speech_level`/`tense`, same as before migration 20240101000026 --
    /// only the joined table changed). Callers should `INNER JOIN targets`
    /// rather than `LEFT JOIN`: unlike the old `sentence_inflection_hints`,
    /// a `targets` row is guaranteed to exist for every sentence (its
    /// `form` is `NOT NULL`), so there's no sparse-row case to `COALESCE`
    /// around here the way there used to be.
    pub fn from_row(row: &sqlx::sqlite::SqliteRow) -> Self {
        use sqlx::Row;
        Self {
            speech_level: row.get("speech_level"),
            tense: row.get("tense"),
            is_honorific: row.get("is_honorific"),
            is_humble: row.get("is_humble"),
        }
    }
}
