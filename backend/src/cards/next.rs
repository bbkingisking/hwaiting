//! `GET /api/cards/next` - selects the next due/new card and shapes it into
//! the pre-answer [`CardPrompt`], withholding everything [`super::CardReveal`]
//! would give away.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};
use utoipa::{IntoParams, ToSchema};

use crate::error::{AppError, AppQuery};

use super::{hanja_hints_for, review_prefs, ReviewPrefs};
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
    /// KRDICT's `ParaWordNo` for this word, when it came from KRDICT.
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
/// `target` is expected to be a literal substring of `sentence` -
/// `admin::edit_card` enforces that on write. If it somehow isn't (e.g. a
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

    let ReviewPrefs { day_boundary_hour, daily_new_card_limit, .. } = review_prefs(&pool, user_id).await?;

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
    // Exclude suppressed cards via users_card_flags
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

    let eng_id = crate::enum_lookup::eng_language_id(&pool).await?;

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
        INNER JOIN cards_translations ct ON c.id = ct.card_id AND ct.language_id = ?
        INNER JOIN sentences s ON c.id = s.card_id
        INNER JOIN targets tg ON tg.sentence_id = s.id
        LEFT JOIN sentences_translations st ON s.id = st.sentence_id AND st.language_id = ?
        LEFT JOIN parts_of_speech pop ON pop.id = c.pos_id
        LEFT JOIN origin_types ot ON ot.id = c.origin_type_id
        LEFT JOIN grades g ON g.id = c.grade_id
        LEFT JOIN speech_levels sl ON sl.id = tg.speech_level_id
        LEFT JOIN tenses tn ON tn.id = tg.tense_id
        LEFT JOIN grammar_patterns gp ON gp.id = tg.grammar_pattern_id
        LEFT JOIN cards_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN users_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (ucf.suppressed IS NULL OR ucf.suppressed = 0)
        {}
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
        .bind(eng_id)
        .bind(eng_id)
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
            INNER JOIN cards_states cs ON cs.card_id = c.id AND cs.user_id = ?
            LEFT JOIN users_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
            WHERE datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days') > datetime('now')
            AND (ucf.suppressed IS NULL OR ucf.suppressed = 0)
            "#,
        )
        .bind(user_id)
        .bind(user_id)
        .fetch_one(&pool)
        .await?;

        let new_card_reset = if new_card_limit_reached {
            let new_cards_waiting: i64 = sqlx::query_scalar(
                r#"
                SELECT EXISTS (
                    SELECT 1 FROM cards c
                    LEFT JOIN cards_states cs ON cs.card_id = c.id AND cs.user_id = ?
                    LEFT JOIN users_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
                    WHERE cs.last_review IS NULL
                    AND (ucf.suppressed IS NULL OR ucf.suppressed = 0)
                )
                "#,
            )
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- split_sentence -----------------------------------------------------

    #[test]
    fn splits_target_at_start() {
        let (before, after) = split_sentence("먹다 좋아해요", "먹다");
        assert_eq!(before, "");
        assert_eq!(after, " 좋아해요");
    }

    #[test]
    fn splits_target_in_middle() {
        let (before, after) = split_sentence("저는 먹다 좋아해요", "먹다");
        assert_eq!(before, "저는 ");
        assert_eq!(after, " 좋아해요");
    }

    #[test]
    fn splits_target_at_end() {
        let (before, after) = split_sentence("저는 먹다", "먹다");
        assert_eq!(before, "저는 ");
        assert_eq!(after, "");
    }

    #[test]
    fn falls_back_to_whole_sentence_when_target_absent() {
        let (before, after) = split_sentence("저는 밥을 먹어요", "먹다");
        assert_eq!(before, "저는 밥을 먹어요");
        assert_eq!(after, "");
    }

    #[test]
    fn splits_at_first_occurrence_when_target_repeats() {
        let (before, after) = split_sentence("가다 가다 가다", "가다");
        assert_eq!(before, "");
        assert_eq!(after, " 가다 가다");
    }

    #[test]
    fn empty_target_splits_at_start() {
        let (before, after) = split_sentence("안녕하세요", "");
        assert_eq!(before, "");
        assert_eq!(after, "안녕하세요");
    }

    // --- deserialize_id_list --------------------------------------------------
    //
    // Called directly with a `StrDeserializer` wrapping the raw `exclude=`
    // value - the same deserializer kind axum's `Query` extractor (via
    // `serde_urlencoded`) hands a `#[serde(deserialize_with = ...)]` field,
    // so this exercises the real code path without pulling in
    // `serde_urlencoded` as a dev-dependency just to build a query string.

    fn parse_list(raw: &str) -> Result<Vec<i64>, serde::de::value::Error> {
        use serde::de::IntoDeserializer;
        let deserializer: serde::de::value::StrDeserializer<'_, serde::de::value::Error> =
            raw.into_deserializer();
        deserialize_id_list(deserializer)
    }

    #[test]
    fn exclude_single_id() {
        assert_eq!(parse_list("42").unwrap(), vec![42]);
    }

    #[test]
    fn exclude_comma_list() {
        assert_eq!(parse_list("1,2,3").unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn exclude_tolerates_spaces_around_commas() {
        assert_eq!(parse_list("1, 2 ,3").unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn exclude_empty_string_is_empty_list() {
        assert_eq!(parse_list("").unwrap(), Vec::<i64>::new());
    }

    #[test]
    fn exclude_trailing_comma_is_silently_ignored() {
        // Empty segments (from a trailing comma, a leading one, or a
        // doubled one) are filtered out before parsing, not rejected -
        // `1,2,` and `1,,2` both silently become `[1, 2]`. Documents the
        // lenient behavior rather than asserting it's the ideal one.
        assert_eq!(parse_list("1,2,").unwrap(), vec![1, 2]);
        assert_eq!(parse_list("1,,2").unwrap(), vec![1, 2]);
    }

    #[test]
    fn exclude_non_numeric_is_rejected() {
        assert!(parse_list("abc").is_err());
    }

    // --- get_next_card (handler-level, in-process SQLite) --------------------

    use crate::auth::AuthUser;
    use crate::cards::time::parse_flexible_datetime;
    use crate::test_support::{test_pool, test_user};

    async fn next_card(pool: &SqlitePool, user_id: i64, exclude: Vec<i64>) -> NextCardEnvelope {
        get_next_card(State(pool.clone()), AuthUser(user_id), AppQuery(NextCardQuery { exclude }))
            .await
            .unwrap()
            .0
    }

    /// Marks every seeded card except `keep` as already-reviewed and due far
    /// in the future - used to isolate a single card as the only "new" (or
    /// only "due-later") candidate without hand-seeding all 50.
    async fn bury_every_other_card(pool: &SqlitePool, user_id: i64, keep: i64) {
        sqlx::query(
            r#"
            INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state)
            SELECT ?, id, 1000, 0, datetime('now'), 'review' FROM cards WHERE id != ?
            "#,
        )
        .bind(user_id)
        .bind(keep)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn due_card_is_preferred_over_new_cards() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;

        // Card 2 is already due (last_review + stability in the past);
        // every other card (including card 1) is untouched, i.e. "new".
        sqlx::query(
            "INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state) \
             VALUES (?, 2, 1, 0, datetime('now', '-10 days'), 'review')",
        )
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

        let envelope = next_card(&pool, user_id, vec![]).await;
        let card = envelope.card.expect("a card should be due");
        assert_eq!(card.prompt.front.card_id, 2);
    }

    #[tokio::test]
    async fn suppressed_card_is_never_returned() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;

        // Card 2 is due, but suppressed - it must not come back even though
        // it would otherwise win on priority.
        sqlx::query(
            "INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state) \
             VALUES (?, 2, 1, 0, datetime('now', '-10 days'), 'review')",
        )
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO users_card_flags (user_id, card_id, suppressed) VALUES (?, 2, 1)")
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap();

        let envelope = next_card(&pool, user_id, vec![]).await;
        let card = envelope.card.expect("some other card should still be available");
        assert_ne!(card.prompt.front.card_id, 2);
    }

    #[tokio::test]
    async fn exclude_param_is_honored() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;

        // Every card is "new" (no cards_states rows) - excluding all but
        // card 7 should deterministically return card 7.
        let exclude: Vec<i64> = (1..=50).filter(|&id| id != 7).collect();
        let envelope = next_card(&pool, user_id, exclude).await;
        let card = envelope.card.expect("card 7 should still be selectable");
        assert_eq!(card.prompt.front.card_id, 7);
    }


    #[tokio::test]
    async fn daily_new_card_limit_zero_suppresses_all_new_cards() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        sqlx::query("INSERT INTO users_settings (user_id, daily_new_card_limit) VALUES (?, 0)")
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap();

        // No card has any review_history, so with new cards suppressed and
        // none due, nothing is available.
        let envelope = next_card(&pool, user_id, vec![]).await;
        assert!(envelope.card.is_none());
    }

    #[tokio::test]
    async fn prefetch_uses_a_stricter_limit_minus_one_threshold() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        sqlx::query("INSERT INTO users_settings (user_id, daily_new_card_limit) VALUES (?, 5)")
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap();

        // Cards 2-5 each have exactly one review, timestamped "now" (today) -
        // 4 new cards reviewed today. A matching cards_states row (as
        // check_answer would also write) pushes each card's next due date
        // 10 days out, so none of them come back as "due" and confound the
        // new-card-limit assertions below.
        for card_id in 2..=5 {
            sqlx::query(
                "INSERT INTO review_history (user_id, card_id, rating, reviewed_at, stability, difficulty, state) \
                 VALUES (?, ?, 'good', datetime('now'), 10, 1, 'learning')",
            )
            .bind(user_id)
            .bind(card_id)
            .execute(&pool)
            .await
            .unwrap();

            sqlx::query(
                "INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state) \
                 VALUES (?, ?, 10, 1, datetime('now'), 'learning')",
            )
            .bind(user_id)
            .bind(card_id)
            .execute(&pool)
            .await
            .unwrap();
        }

        // Plain request: threshold is the full limit (5); 4 < 5, so a new
        // card is still available.
        let plain = next_card(&pool, user_id, vec![]).await;
        assert!(plain.card.is_some(), "limit not yet reached for a plain request");

        // Prefetch request (non-empty exclude): threshold is limit - 1 (4);
        // 4 >= 4, so the prefetch must not hand out a new card even though
        // the real limit hasn't been hit yet.
        let prefetch = next_card(&pool, user_id, vec![999]).await;
        assert!(prefetch.card.is_none(), "prefetch should respect the limit-1 buffer");
    }

    #[tokio::test]
    async fn next_due_at_reports_the_earliest_scheduled_due_date() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;

        // Every card has a cards_states row (so none reads as "new"), and
        // card 1's is due soonest.
        bury_every_other_card(&pool, user_id, 1).await;
        sqlx::query(
            "INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state) \
             VALUES (?, 1, 3, 0, datetime('now'), 'review')",
        )
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

        let last_review: String = sqlx::query_scalar(
            "SELECT last_review FROM cards_states WHERE user_id = ? AND card_id = 1",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let expected = (parse_flexible_datetime(&last_review).unwrap() + chrono::Duration::days(3))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();

        let envelope = next_card(&pool, user_id, vec![]).await;
        assert!(envelope.card.is_none(), "nothing should be due yet");
        assert_eq!(envelope.next_due_at, Some(expected));
    }
}
