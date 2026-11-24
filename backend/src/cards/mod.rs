//! The review flow: selecting the next card, grading an answer against it,
//! and the stats/moderation surfaces built on top of the same tables. Split
//! by concern - see each submodule's doc comment - with this file limited to
//! wiring plus the couple of types/helpers genuinely shared across more than
//! one of them.

pub(crate) mod attempt;
mod check;
mod fsrs_admin;
mod grammar_patterns;
mod hanja_drill;
mod moderation;
mod next;
pub(crate) mod scheduling;
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
pub(crate) use attempt::*;
pub(crate) use check::*;
pub(crate) use fsrs_admin::*;
pub(crate) use grammar_patterns::*;
pub(crate) use hanja_drill::*;
pub(crate) use moderation::*;
pub(crate) use next::*;
pub(crate) use stats::*;
pub(crate) use time::{parse_flexible_datetime, rfc3339, sqlite_datetime};

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

/// Availability preferences shared by queue selection and statistics.
/// Scheduling's model/retention are loaded separately under its transaction.
#[derive(sqlx::FromRow)]
pub(crate) struct ReviewPrefs {
    pub day_boundary_hour: i64,
    pub study_timezone: Option<String>,
    pub daily_new_card_limit: i64,
}

/// Loads `ReviewPrefs` for `user_id`, falling back to this app's defaults -
/// matching `users_settings`' own column `DEFAULT`s exactly - for a user who
/// has never hit `GET/PATCH /api/user/settings` yet and so has no row at
/// all. Deliberately read-only: unlike `user::ensure_settings_row`, this
/// never materializes the row itself, since none of `review_prefs`' callers
/// otherwise have a reason to write one - it's fine for a brand-new account
/// to keep getting these defaults rather than have a row appear as a side
/// effect of a `GET`.
pub(crate) async fn review_prefs(pool: &SqlitePool, user_id: i64) -> Result<ReviewPrefs, AppError> {
    let prefs = sqlx::query_as::<_, ReviewPrefs>(
        "SELECT day_boundary_hour, daily_new_card_limit, study_timezone FROM users_settings WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(prefs.unwrap_or(ReviewPrefs {
        study_timezone: None,
        day_boundary_hour: 4,
        daily_new_card_limit: 20,
    }))
}

#[derive(Serialize, Clone, ToSchema)]
pub struct HanjaHint {
    pub hanja: String,
    pub word: String,
    #[schema(required)]
    pub trans_word: Option<String>,
}

/// Localized card projection used internally by the scoped edit writer.
/// Public admin reads use AdminCardDetail, which contains the sentence
/// and all translations rather than selecting one language.
#[derive(Serialize, ToSchema)]
pub struct Card {
    #[serde(flatten)]
    pub front: CardFront,
    #[serde(flatten)]
    pub back: CardBack,
}

/// A resolved conjugation with its grammatical facts. Clients own labels,
/// grouping and ordering; the backend returns only forms applicable to the card.
#[derive(Serialize, ToSchema)]
pub struct CardInflection {
    pub form_slug: String,
    pub form: String,
    pub ending: String,
    #[schema(required)]
    pub tense: Option<String>,
    #[schema(required)]
    pub speech_level: Option<String>,
}

/// This card's resolved `conjugation_matrix_cards` rows, joined out to the
/// catalog's `slug`, for the standalone admin conjugation-table lookup
/// (`admin::get_card_inflections`). Review responses do not carry this matrix.
/// `pub(crate)`, unlike `hanja_hints_on_connection` below, because `admin` is a sibling
/// module rather than a submodule of `cards` and can't otherwise reach it.
///
/// Generic over the executor rather than fixed to `&SqlitePool`, so
/// `admin::get_card_inflections` can run this inside the same transaction as
/// its existence check - closing the window a card deleted between two
/// separate pool queries used to open, where the delete would land between
/// the check and this fetch and the endpoint would return 200 with an empty
/// list instead of 404.
pub(crate) async fn inflections_for<'e, E>(
    executor: E,
    card_id: i64,
) -> Result<Vec<CardInflection>, AppError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    Ok(sqlx::query(
        r#"
        SELECT f.slug as form_slug, cmc.form, f.ending, tn.slug AS tense, sl.slug AS speech_level
        FROM conjugation_matrix_cards cmc
        JOIN conjugation_matrix_forms f ON cmc.form_id = f.id
        JOIN cards c ON c.id = cmc.card_id
        LEFT JOIN tenses tn ON tn.id = f.tense_id
        LEFT JOIN speech_levels sl ON sl.id = f.speech_level_id
        WHERE cmc.card_id = ?
          AND (f.restricted_to_pos_id IS NULL OR f.restricted_to_pos_id = c.pos_id)
        ORDER BY f.slug
        "#,
    )
    .bind(card_id)
    .fetch_all(executor)
    .await?
    .iter()
    .map(|row| CardInflection {
        form_slug: row.get("form_slug"),
        form: row.get("form"),
        ending: row.get("ending"),
        tense: row.get("tense"),
        speech_level: row.get("speech_level"),
    })
    .collect())
}

/// Whether `a` and `b` share at least one CJK Unified Ideograph (U+4E00 to
/// U+9FFF - the "CJK Unified Ideographs" block proper). Hangul-only strings,
/// or ones with no overlap, never match. Note this range excludes CJK
/// Extension A (U+3400 to U+4DBF) and the CJK Compatibility Ideographs block
/// (U+F900 to U+FAFF) - a hanja drawn from either would never register as
/// shared here even if it's the identical character KRDICT records
/// elsewhere as canonically decomposing to one already in range. Extracted
/// from `hanja_hints_on_connection` so the character-matching rule can be tested
/// without a database.
fn shares_hanja_character(a: &str, b: &str) -> bool {
    let a_chars: std::collections::HashSet<char> = a
        .chars()
        .filter(|c| ('\u{4E00}'..='\u{9FFF}').contains(c))
        .collect();
    if a_chars.is_empty() {
        return false;
    }
    b.chars()
        .filter(|c| ('\u{4E00}'..='\u{9FFF}').contains(c))
        .any(|c| a_chars.contains(&c))
}

/// Hanja hints for `card_id`: hanja from other cards the user has already
/// reviewed that share at least one character with `hanja`. Shared by
/// `get_review_queue` (which only needs the characters - see
/// `ReviewContext::hanja_hint_words`) and `check_answer` (which needs the full
/// reading/gloss once the card is graded - see `CardReveal::hanja_hints`).
async fn hanja_hints_on_connection(
    connection: &mut sqlx::SqliteConnection,
    eng_id: i64,
    user_id: i64,
    card_id: i64,
    hanja: &Option<String>,
) -> Result<Vec<HanjaHint>, AppError> {
    let Some(current_hanja) = hanja.as_ref().filter(|h| !h.is_empty()) else {
        return Ok(vec![]);
    };
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
        "#,
    )
    .bind(eng_id)
    .bind(user_id)
    .bind(card_id)
    .fetch_all(connection)
    .await?;

    Ok(other_hanja_rows
        .iter()
        .filter_map(|row| {
            let other_hanja: String = row.get("hanja");
            if shares_hanja_character(current_hanja, &other_hanja) {
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

#[cfg(test)]
#[path = "../tests/cards/mod_tests.rs"]
mod tests;
