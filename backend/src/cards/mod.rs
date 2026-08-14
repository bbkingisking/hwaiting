//! The review flow: selecting the next card, grading an answer against it,
//! and the stats/moderation surfaces built on top of the same tables. Split
//! by concern - see each submodule's doc comment - with this file limited to
//! wiring plus the couple of types/helpers genuinely shared across more than
//! one of them.

mod check;
mod fsrs_admin;
mod lookups;
mod moderation;
mod next;
mod stats;
mod time;

// Globbed rather than named: `#[utoipa::path]` on each handler generates a
// same-visibility sibling `__path_<fn>` type that `openapi.rs`'s `paths(...)`
// needs but never names directly, so an explicit re-export list would miss
// it. `pub(crate)` throughout because nothing outside this crate consumes
// `cards::*` (this is a bin crate) - which also lets `next::split_sentence`,
// itself `pub(crate)` for the same reason, re-export cleanly instead of
// tripping the "can't re-export at wider visibility" error a plain `pub use`
// would hit.
pub(crate) use check::*;
pub(crate) use fsrs_admin::*;
pub(crate) use lookups::*;
pub(crate) use moderation::*;
pub(crate) use next::*;
pub(crate) use stats::*;

use serde::Serialize;
use sqlx::{Row, SqlitePool};
use utoipa::ToSchema;

use crate::error::AppError;

#[derive(Serialize, Clone, ToSchema)]
pub struct HanjaHint {
    pub hanja: String,
    pub hanja_eum: Option<String>,
    pub trans_word: Option<String>,
}

/// The canonical full-card shape: a `cards` row joined with its English
/// translation and primary example sentence. Shared verbatim by admin
/// search (`admin::search_cards`) and edit (`admin::edit_card`) - previously
/// two independently hand-declared structs that happened to agree on 14 of
/// their fields, which is exactly the kind of duplication that drifts
/// silently over time. Not used by the review flow, which only ever needs
/// the `CardPrompt`/`CardReveal` split (see `next`/`check`) - admin editing
/// isn't gated by the same secrecy concerns, so it gets the whole row upfront.
#[derive(Serialize, ToSchema)]
pub struct Card {
    pub card_id: i64,
    /// KRDICT's `ParaWordNo` for this word, when it came from KRDICT. `None`
    /// for user-created custom cards, which have no upstream dictionary entry.
    pub krdict_id: Option<i64>,
    pub word: String,
    pub definition: Option<String>,
    pub pos: Option<String>,
    pub origin_type: Option<String>,
    pub hanja: Option<String>,
    pub hanja_eum: Option<String>,
    pub grade: Option<String>,
    pub trans_word: String,
    pub trans_dfn: Option<String>,
    pub sentence: String,
    /// `sentence`, sliced at `target`'s position: the text before the blank.
    /// Derived once here rather than by every renderer re-searching
    /// `sentence` for `target` - see `split_sentence`.
    pub sentence_before: String,
    /// The text after the blank. See `sentence_before`.
    pub sentence_after: String,
    pub sentence_translation: String,
    pub target: String,
    pub alternatives: Vec<String>,
    pub speech_level: Option<String>,
    pub tense: Option<String>,
    pub grammar_pattern: Option<String>,
}

/// Hanja hints for `card_id`: hanja from other cards the user has already
/// reviewed that share at least one character with `hanja`. Shared by
/// `get_next_card` (which only needs the characters - see
/// `CardPrompt::hanja_hint_words`) and `check_answer` (which needs the full
/// reading/gloss once the card is graded - see `CardReveal::hanja_hints`).
async fn hanja_hints_for(
    pool: &SqlitePool,
    user_id: i64,
    card_id: i64,
    hanja: &Option<String>,
) -> Result<Vec<HanjaHint>, AppError> {
    let Some(current_hanja) = hanja else { return Ok(vec![]) };
    if current_hanja.is_empty() {
        return Ok(vec![]);
    }

    let other_hanja_rows = sqlx::query(
        r#"
        SELECT DISTINCT c.hanja, c.hanja_eum, ct.trans_word
        FROM card_states cs
        INNER JOIN cards c ON c.id = cs.card_id
        INNER JOIN card_translations ct ON ct.card_id = c.id AND ct.language_tag = 'en'
        WHERE cs.user_id = ?
          AND cs.card_id != ?
          AND c.hanja IS NOT NULL
          AND c.hanja != ''
        "#
    )
    .bind(user_id)
    .bind(card_id)
    .fetch_all(pool)
    .await?;

    let current_chars: std::collections::HashSet<char> =
        current_hanja.chars().filter(|c| ('\u{4E00}'..='\u{9FFF}').contains(c)).collect();

    Ok(other_hanja_rows
        .iter()
        .filter_map(|row| {
            let other_hanja: String = row.get("hanja");
            let other_chars: std::collections::HashSet<char> =
                other_hanja.chars().filter(|c| ('\u{4E00}'..='\u{9FFF}').contains(c)).collect();
            if !current_chars.is_empty() && !other_chars.is_empty() && current_chars.intersection(&other_chars).next().is_some() {
                Some(HanjaHint {
                    hanja: other_hanja,
                    hanja_eum: row.get("hanja_eum"),
                    trans_word: row.get("trans_word"),
                })
            } else {
                None
            }
        })
        .collect())
}
