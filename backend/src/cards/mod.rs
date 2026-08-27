//! The review flow: selecting the next card, grading an answer against it,
//! and the stats/moderation surfaces built on top of the same tables. Split
//! by concern - see each submodule's doc comment - with this file limited to
//! wiring plus the couple of types/helpers genuinely shared across more than
//! one of them.

mod check;
mod field_values;
mod fsrs_admin;
mod hanja_drill;
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
pub(crate) use field_values::*;
pub(crate) use fsrs_admin::*;
pub(crate) use hanja_drill::*;
pub(crate) use moderation::*;
pub(crate) use next::*;
pub(crate) use stats::*;

use serde::Serialize;
use sqlx::{Row, Sqlite, SqlitePool};
use utoipa::ToSchema;

use crate::error::AppError;

/// The `cards_states.state` value marking a card as mastered - past initial
/// learning/relearning, in FSRS's long-interval review phase. Shared by
/// `stats::query_summary`'s "Mastered" stat and `hanja_drill::get_hanja_drill`'s
/// eligible-card pool, so the two can't silently drift apart on what counts
/// as mastered - the same must-agree concern `cards/time.rs`'s
/// `COUNTED_REVIEW_SQL`/`CORRECT_REVIEW_SQL` document for accuracy stats.
pub(crate) const MASTERED_STATE: &str = "review";

#[derive(Serialize, Clone, ToSchema)]
pub struct HanjaHint {
    pub hanja: String,
    pub word: String,
    pub trans_word: Option<String>,
}

/// The canonical full-card shape: a `cards` row joined with its English
/// translation and primary example sentence - exactly `CardFront` (next.rs)
/// plus `CardBack` (check.rs), i.e. exactly what `CardPrompt` and
/// `CardReveal` disclose between them minus the two fields that are
/// review-flow state rather than properties of the card itself
/// (`hanja_hint_words`, `hanja_hints` - both depend on the requesting user's
/// review history, see `hanja_hints_for`).
///
/// Flattening the same two structs the review flow already produces, rather
/// than hand-declaring their union a third time, is what keeps this
/// in sync with them: `Card` used to be its own independently-declared
/// 20-field struct that merely happened to agree with `CardPrompt`/
/// `CardReveal` - previously it was also independent of `admin::AdminCard`,
/// which had the exact same problem in miniature (agreeing on 14 of its own
/// fields) until both admin structs were unified into this one. Nothing
/// enforced this wider agreement in the same way; the frontend's
/// `toAdminCard`/`CARD_BACK_FIELDS` (lib/api.ts) had to hand-encode the
/// front/back split from the outside, reconstructing in TypeScript what the
/// Rust types could just express directly.
///
/// Shared verbatim by admin search (`admin::search_cards`) and edit
/// (`admin::edit_card`). Not used by the review flow itself, which needs
/// `CardPrompt`/`CardReveal` proper, extra per-user fields included -
/// admin editing isn't gated by the same secrecy concerns, so it gets the
/// whole row upfront.
#[derive(Serialize, ToSchema)]
pub struct Card {
    #[serde(flatten)]
    pub front: CardFront,
    #[serde(flatten)]
    pub back: CardBack,
}

/// This card's resolved `conjugation_matrix_cards` rows, joined out to the
/// catalog's `slug` - shared by `check_answer` (`CardReveal::inflections`,
/// which needs it alongside the rest of a graded reveal) and admin's
/// standalone per-card lookup (`admin::get_card_inflections`, which needs
/// exactly these rows without going through the review-grading flow).
/// `pub(crate)`, unlike `hanja_hints_for` below, because `admin` is a sibling
/// module rather than a submodule of `cards` and can't otherwise reach it.
///
/// Generic over the executor (rather than fixed to `&SqlitePool`, following
/// `enum_lookup`'s `eng_language_id`/`resolve_optional_id`) so
/// `admin::get_card_inflections` can run this inside the same transaction as
/// its existence check - closing the window a card deleted between two
/// separate pool queries used to open, where the delete would land between
/// the check and this fetch and the endpoint would return 200 with an empty
/// list instead of 404.
pub(crate) async fn inflections_for<'e, E>(executor: E, card_id: i64) -> Result<Vec<CardInflection>, AppError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    Ok(sqlx::query(
        r#"
        SELECT f.slug as form_slug, cmc.form
        FROM conjugation_matrix_cards cmc
        JOIN conjugation_matrix_forms f ON cmc.form_id = f.id
        WHERE cmc.card_id = ?
        ORDER BY f.sort_order
        "#,
    )
    .bind(card_id)
    .fetch_all(executor)
    .await?
    .iter()
    .map(|row| CardInflection {
        form_slug: row.get("form_slug"),
        form: row.get("form"),
    })
    .collect())
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

    let eng_id = crate::enum_lookup::eng_language_id(pool).await?;
    let other_hanja_rows = sqlx::query(
        r#"
        SELECT DISTINCT c.hanja, c.word, ct.trans_word
        FROM cards_states cs
        INNER JOIN cards c ON c.id = cs.card_id
        INNER JOIN cards_translations ct ON ct.card_id = c.id AND ct.language_id = ?
        WHERE cs.user_id = ?
          AND cs.card_id != ?
          AND c.hanja IS NOT NULL
          AND c.hanja != ''
        "#
    )
    .bind(eng_id)
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
                    word: row.get("word"),
                    trans_word: row.get("trans_word"),
                })
            } else {
                None
            }
        })
        .collect())
}
