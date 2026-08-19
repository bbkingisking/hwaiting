//! `GET /api/cards/next` - selects the next due/new card and shapes it into
//! the pre-answer [`CardPrompt`], withholding everything [`super::CardReveal`]
//! would give away.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};
use utoipa::{IntoParams, ToSchema};

use crate::error::{AppError, AppQuery};

use super::hanja_hints_for;
use super::time::{logical_today_start, sqlite_datetime};

/// The card fields visible before an answer is checked, shared with the
/// admin-editing shape - see `cards::Card`'s doc comment - and with
/// `CardBack` (check.rs) for the withheld half. Split out from `CardPrompt`
/// (below) so `Card` can flatten this struct instead of hand-declaring the
/// same 13 fields a third time: `Card` used to be its own standing struct
/// that merely happened to agree with `CardPrompt`'s, the exact
/// silent-drift risk `Card`'s doc comment already flags from its earlier
/// history with the admin struct it replaced.
///
/// `definition` and the unsliced `sentence` are withheld too, even though
/// neither is rendered by the review UI at all pre- or post-answer - they're
/// authoring fields, not review-flow fields. `CardBack` carries them anyway,
/// purely so an admin editing a card mid-review has a correct, non-blank
/// baseline to save over.
#[derive(Serialize, ToSchema)]
pub struct CardFront {
    pub card_id: i64,
    /// KRDICT's `ParaWordNo` for this word, when it came from KRDICT. `None`
    /// for user-created custom cards, which have no upstream dictionary entry.
    pub krdict_id: Option<i64>,
    pub pos: Option<String>,
    pub origin_type: Option<String>,
    pub grade: Option<String>,
    pub trans_word: String,
    pub trans_dfn: Option<String>,
    /// `sentence`, sliced at `target`'s position: the text before the blank.
    /// Derived once here rather than by every renderer re-searching
    /// `sentence` for `target` - see `split_sentence`. The unsliced
    /// `sentence` and `target` itself are withheld; see `CardBack`.
    pub sentence_before: String,
    /// The text after the blank. See `sentence_before`.
    pub sentence_after: String,
    pub sentence_translation: String,
    #[serde(flatten)]
    pub inflection_hint: crate::inflection_hints::InflectionHint,
    pub grammar_pattern: Option<String>,
    pub hanja: Option<String>,
}

/// Everything the client may see before it has attempted an answer:
/// `CardFront` plus `hanja_hint_words`, the one field here that's genuinely
/// review-flow-specific rather than a property of the card itself - it
/// depends on the requesting user's review history (see `hanja_hints_for`),
/// so it has no place on `CardFront`/`Card`. Served by `GET /api/cards/next`.
#[derive(Serialize, ToSchema)]
pub struct CardPrompt {
    #[serde(flatten)]
    pub front: CardFront,
    /// Hanja characters for the pre-answer hint span. The reading and each
    /// hint's gloss give the answer away - the reading is `CardBack::word`
    /// itself (Korean orthography is phonetic, so a word's spelling and its
    /// hanja's reading are the same fact) - see also `HanjaHint::trans_word`.
    pub hanja_hint_words: Vec<String>,
}

/// Split `sentence` into the text before and after `target`, so callers can
/// render the sentence with `target` blanked out without needing to know
/// where it sits. This is the one place that does that search: every render
/// site used to redo `sentence.indexOf(target)` itself (and disagreed, in
/// one case silently, about what to do when `target` isn't found).
///
/// `target` is expected to be a literal substring of `sentence` - both
/// `custom_cards::create_custom_card`/`update_custom_card` and
/// `admin::edit_card` enforce that on write. If it somehow isn't (e.g. a
/// pre-validation row), fall back to the whole sentence with no blank rather
/// than panicking or hiding the sentence.
pub(crate) fn split_sentence(sentence: &str, target: &str) -> (String, String) {
    match sentence.find(target) {
        Some(idx) => (
            sentence[..idx].to_string(),
            sentence[idx + target.len()..].to_string(),
        ),
        None => (sentence.to_string(), String::new()),
    }
}

#[derive(Serialize, ToSchema)]
pub struct NextCardResponse {
    #[serde(flatten)]
    prompt: CardPrompt,
    difficulty: Option<f64>,
    guess_count: i64,
    wrong_guess_count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct NextCardEnvelope {
    pub card: Option<NextCardResponse>,
    pub next_due_at: Option<String>,
}

#[derive(Deserialize, Default, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct NextCardQuery {
    /// Comma-separated card ids to exclude from the result. The frontend's
    /// prefetch sends exactly one (the card currently on screen); the
    /// hwaiting-agent CLI sends its whole local set of already-claimed
    /// cards, so that concurrent agent processes don't get handed a card
    /// someone else already has. `serde_urlencoded` (what axum's `Query`
    /// extractor uses) has no support for repeated-key arrays, hence the
    /// comma-joined string instead of `exclude=1&exclude=2`. `explode =
    /// false` records that in the schema too, so generated clients send
    /// `exclude=1,2,3` (style: form, explode: false) instead of the
    /// OpenAPI-default repeated-key form this endpoint can't parse.
    #[serde(default, deserialize_with = "deserialize_id_list")]
    #[param(explode = false)]
    exclude: Vec<i64>,
}

fn deserialize_id_list<'de, D>(deserializer: D) -> Result<Vec<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    raw.split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().parse::<i64>().map_err(serde::de::Error::custom))
        .collect()
}

// Get next card due for review
#[utoipa::path(
    get,
    path = "/api/cards/next",
    params(NextCardQuery),
    responses(
        (status = 200, description = "Next due/new card, or null if none due", body = NextCardEnvelope),
        (status = 400, description = "Malformed query string", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn get_next_card(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
    AppQuery(params): AppQuery<NextCardQuery>,
) -> Result<Json<NextCardEnvelope>, AppError> {
    let user_id = auth.0;
    info!(
        "Getting next card for user_id: {} (exclude: {:?})",
        user_id, params.exclude
    );

    // Get user settings
    let user_row = sqlx::query(
        "SELECT daily_new_card_limit, day_boundary_hour FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?;

    let daily_new_card_limit = user_row
        .as_ref()
        .and_then(|r| r.get::<Option<i64>, _>("daily_new_card_limit"))
        .unwrap_or(20);

    let day_boundary_hour = user_row
        .as_ref()
        .and_then(|r| r.get::<Option<i64>, _>("day_boundary_hour"))
        .unwrap_or(4);

    // Start of "today" per day_boundary_hour - same helper get_stats uses.
    let today_start_str = sqlite_datetime(logical_today_start(day_boundary_hour));

    // Count how many NEW cards the user has reviewed today
    // A "new" card is one where it's the user's first review (no prior review_history)
    // Check if new cards are suppressed (limit = 0) or if daily limit is reached
    let new_card_limit_reached = if daily_new_card_limit == 0 {
        true  // Suppress all new cards
    } else {
        // Count how many NEW cards the user has reviewed today
        let new_cards_today: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(DISTINCT rh.card_id)
            FROM review_history rh
            WHERE rh.user_id = ?
            AND rh.reviewed_at >= ?
            AND NOT EXISTS (
                SELECT 1 FROM review_history rh2
                WHERE rh2.user_id = rh.user_id
                AND rh2.card_id = rh.card_id
                AND rh2.reviewed_at < ?
            )
            "#
        )
        .bind(user_id)
        .bind(&today_start_str)
        .bind(&today_start_str)
        .fetch_one(&pool)
        .await?;

        // For prefetch requests (exclude param is non-empty), use stricter limit to prevent race condition.
        // When the user is on card N (new card #19/20), the prefetch for N+1 should not return
        // a new card because by the time N+1 is displayed, card N will have been reviewed,
        // pushing the count to 20/20 and making N+1 display as 21/20.
        // For normal requests, use the actual limit.
        let is_prefetch = !params.exclude.is_empty();
        let threshold = if is_prefetch {
            daily_new_card_limit - 1  // Block at limit-1 for prefetch
        } else {
            daily_new_card_limit  // Block at limit for normal fetch
        };

        new_cards_today >= threshold
    };

    // Get next due card (prioritize due cards by due date, then new cards)
    // Exclude suppressed cards via user_card_flags
    // Optionally skip a set of card_ids (client-side prefetch skips the
    // card on screen; hwaiting-agent skips every card it knows is already
    // claimed by a sibling process)
    // When daily new card limit is 0 or reached (including limit-1 buffer), only show cards that have been reviewed before
    let exclude_clause = if params.exclude.is_empty() {
        String::new()
    } else {
        format!(
            "AND c.id NOT IN ({})",
            params.exclude.iter().map(|_| "?").collect::<Vec<_>>().join(",")
        )
    };

    let new_card_filter = if new_card_limit_reached {
        // If limit is 0 or reached, only show cards that have been reviewed before (have review history)
        "AND EXISTS (SELECT 1 FROM review_history WHERE card_id = c.id AND user_id = ?)"
    } else {
        ""
    };

    let query = format!(
        r#"
        SELECT
            c.id, c.krdict_id, c.word, c.definition, c.hanja,
            pop.slug as pos, ot.slug as origin_type, g.slug as grade,
            ct.trans_word, ct.trans_dfn,
            s.id as sentence_id, s.text as sentence, tg.form as target,
            st.translation as sentence_translation,
            sl.slug as speech_level, tn.slug as tense,
            tg.is_honorific, tg.is_humble,
            gp.slug as grammar_pattern,
            cs.difficulty, cs.last_review, cs.stability
        FROM cards c
        LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        INNER JOIN card_translations ct ON c.id = ct.card_id AND ct.language_tag = 'en'
        INNER JOIN sentences s ON c.id = s.card_id
        INNER JOIN targets tg ON tg.sentence_id = s.id
        LEFT JOIN sentence_translations st ON s.id = st.sentence_id
        LEFT JOIN parts_of_speech pop ON pop.id = c.pos_id
        LEFT JOIN origin_types ot ON ot.id = c.origin_type_id
        LEFT JOIN grades g ON g.id = c.grade_id
        LEFT JOIN speech_levels sl ON sl.id = tg.speech_level_id
        LEFT JOIN tenses tn ON tn.id = tg.tense_id
        LEFT JOIN grammar_patterns gp ON gp.id = c.grammar_pattern_id
        LEFT JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
        {}
        AND (ucf.suppressed IS NULL OR ucf.suppressed = 0)
        {}
        AND (
            cs.last_review IS NULL
            OR datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days') <= datetime('now')
        )
        ORDER BY
            CASE WHEN cs.last_review IS NULL THEN 1 ELSE 0 END,
            datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days') ASC,
            c.frequency_rank ASC NULLS LAST,
            RANDOM()
        LIMIT 1
        "#,
        new_card_filter, exclude_clause
    );

    let mut query_builder = sqlx::query(&query)
        .bind(user_id)
        .bind(user_id)
        .bind(user_id);

    // Add extra bind for the new card filter if limit is reached
    if new_card_limit_reached {
        query_builder = query_builder.bind(user_id);
    }

    for id in &params.exclude {
        query_builder = query_builder.bind(id);
    }

    let row = query_builder.fetch_optional(&pool).await?;

    let Some(row) = row else {
        // No card available. Two independent things can be blocking, and
        // whichever unblocks first is the honest answer:
        //
        // 1. Every card the user has already reviewed at least once is
        //    scheduled for later - the query below finds the earliest such
        //    due date.
        // 2. The daily new-card cap is reached (new_card_limit_reached),
        //    *and* there's at least one never-reviewed card waiting behind
        //    it - in which case the cap resetting at the next day boundary
        //    is also a candidate. Previously this case fell through to
        //    `next_due_at: None` with no indication of when to come back,
        //    which is the common case for a session that reviews until
        //    there's nothing left to do for the day, rather than one that
        //    stops because everything's genuinely scheduled for later.
        let scheduled_next: Option<String> = sqlx::query_scalar(
            r#"
            SELECT strftime('%Y-%m-%dT%H:%M:%SZ', MIN(datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days')))
            FROM cards c
            LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
            INNER JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
            LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
            WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
            AND datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days') > datetime('now')
            AND (ucf.suppressed IS NULL OR ucf.suppressed = 0)
            "#,
        )
        .bind(user_id)
        .bind(user_id)
        .bind(user_id)
        .fetch_one(&pool)
        .await?;

        let new_card_reset = if new_card_limit_reached {
            let new_cards_waiting: i64 = sqlx::query_scalar(
                r#"
                SELECT EXISTS (
                    SELECT 1 FROM cards c
                    LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
                    LEFT JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
                    LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
                    WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
                    AND cs.last_review IS NULL
                    AND (ucf.suppressed IS NULL OR ucf.suppressed = 0)
                )
                "#,
            )
            .bind(user_id)
            .bind(user_id)
            .bind(user_id)
            .fetch_one(&pool)
            .await?;

            (new_cards_waiting > 0).then(|| logical_today_start(day_boundary_hour) + chrono::Duration::days(1))
        } else {
            None
        };

        let scheduled_next_dt = scheduled_next
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        let next_due_at = [scheduled_next_dt, new_card_reset]
            .into_iter()
            .flatten()
            .min()
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string());

        return Ok(Json(NextCardEnvelope {
            card: None,
            next_due_at,
        }));
    };

    let card_id: i64 = row.get("id");
    let krdict_id: Option<i64> = row.get("krdict_id");
    let word: String = row.get("word");
    let pos: Option<String> = row.get("pos");
    let origin_type: Option<String> = row.get("origin_type");
    let hanja: Option<String> = row.get("hanja");
    let grade: Option<String> = row.get("grade");
    let trans_word: String = row.get("trans_word");
    let trans_dfn: Option<String> = row.get("trans_dfn");
    let sentence: String = row.get("sentence");
    let sentence_translation: String = row.get("sentence_translation");
    let target: String = row.get("target");
    let inflection_hint = crate::inflection_hints::InflectionHint::from_row(&row);
    let grammar_pattern: Option<String> = row.get("grammar_pattern");

    debug!("Selected card_id: {} ({})", card_id, word);

    // Get correct/wrong stats for this card
    let stats_row = sqlx::query(
        r#"
        SELECT
            COUNT(*) as total,
            SUM(CASE WHEN rating IN ('good', 'easy') THEN 1 ELSE 0 END) as correct
        FROM review_history
        WHERE user_id = ? AND card_id = ?
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .fetch_one(&pool)
    .await?;

    let guess_count: i64 = stats_row.get("total");
    let correct_count: i64 = stats_row.get("correct");
    let wrong_guess_count = guess_count - correct_count;

    // Get difficulty from FSRS (range 1-10)
    let difficulty: Option<f64> = if guess_count > 0 {
        row.get("difficulty")
    } else {
        None
    };

    // Pre-answer, only the hanja characters themselves are shown (see
    // CardPrompt::hanja_hint_words) - the reading/gloss on each hint is
    // withheld the same as the card's own `target`, so `check_answer` below
    // recomputes the full hints once the card is graded rather than us
    // shipping them now.
    let hanja_hints = hanja_hints_for(&pool, user_id, card_id, &hanja).await?;
    let hanja_hint_words: Vec<String> = hanja_hints.into_iter().map(|h| h.hanja).collect();

    let (sentence_before, sentence_after) = split_sentence(&sentence, &target);

    Ok(Json(NextCardEnvelope {
        card: Some(NextCardResponse {
            prompt: CardPrompt {
                front: CardFront {
                    card_id,
                    krdict_id,
                    pos,
                    origin_type,
                    grade,
                    trans_word,
                    trans_dfn,
                    sentence_before,
                    sentence_after,
                    sentence_translation,
                    inflection_hint,
                    grammar_pattern,
                    hanja,
                },
                hanja_hint_words,
            },
            difficulty,
            guess_count,
            wrong_guess_count,
        }),
        next_due_at: None,
    }))
}
