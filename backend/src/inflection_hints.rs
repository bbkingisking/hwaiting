//! The shape of a target's tagged grammatical facts, shared across every
//! surface that reads or writes the `targets` table (see migration
//! 20240101000026 - `speech_level_id`/`tense_id`/`is_honorific`/
//! `is_humble` all live on that table, alongside the target's own `form`).
//! Previously hand-declared per-struct at each of `cards::CardFront`,
//! `custom_cards::CustomCard`, and export_import's now-deleted
//! `InflectionHintExport` (and independently again as request fields on the
//! create/update side) -- the exact "same shape, agreeing on every field
//! until one drifts" risk `custom_cards::CustomCard`'s own doc comment
//! already flags for a sibling field. `is_honorific`/`is_humble` (added by
//! migration 20240101000025) are the two fields most likely to repeat that
//! history if left to be added separately at each call site, so they're
//! introduced here once instead.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Read shape: a tagged sentence's hints once resolved from their
/// lookup-table ids back to slugs. `#[serde(flatten)]`ed into every
/// response that carries a sentence's hints, so the wire shape (four flat
/// top-level fields) is unchanged from before `is_honorific`/`is_humble`
/// existed. Also used directly (nested, not flattened) as
/// `export_import::SentenceExport::inflection_hint`.
#[derive(Serialize, Deserialize, Clone, Default, ToSchema)]
pub struct InflectionHint {
    pub speech_level: Option<String>,
    pub tense: Option<String>,
    /// Subject honorification (주체높임, -(으)시-). Independent of
    /// `is_humble` rather than sharing a lookup table with it (the way
    /// `speech_level`/`tense` share theirs) because a target isn't limited
    /// to at most one of the two -- see migration 20240101000025.
    #[serde(default)]
    pub is_honorific: bool,
    /// Object honorification (객체높임/겸양법 -- 드리다, 뵙다, 여쭙다,
    /// 모시다-type suppletive verbs). See migration 20240101000025.
    #[serde(default)]
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

/// Write shape: what a create/update request carries for a sentence's
/// hints, when the endpoint doesn't need to distinguish "omit" from
/// "explicit null" (contrast `admin::UpdateCardRequest`, which does and so
/// declares its own four fields with the double-option pattern instead of
/// flattening this). Shared by `custom_cards::CreateCustomCardRequest` and
/// `custom_cards::UpdateCustomCardRequest`, which already agreed on this
/// exact shape before `is_honorific`/`is_humble` existed.
#[derive(Deserialize, Clone, Default, ToSchema)]
pub struct InflectionHintWrite {
    pub speech_level: Option<String>,
    pub tense: Option<String>,
    pub is_honorific: Option<bool>,
    pub is_humble: Option<bool>,
}
