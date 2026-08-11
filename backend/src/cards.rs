use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use chrono::{Local, TimeZone, Timelike, Utc};
use fsrs::{ComputeParametersInput, FSRSItem, FSRSReview, MemoryState, FSRS, DEFAULT_PARAMETERS};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};
use utoipa::{IntoParams, ToSchema};

use crate::error::{AppError, AppJson, AppPath, AppQuery};

#[derive(Serialize, Clone, ToSchema)]
pub struct HanjaHint {
    pub hanja: String,
    pub hanja_eum: Option<String>,
    pub trans_word: Option<String>,
}

/// Everything the client may see before it has attempted an answer: enough
/// to render the sentence-with-blank, badges, and both translations, but
/// nothing `target` could be inferred from. Served by `GET /api/cards/next`.
///
/// `definition` and the unsliced `sentence` are withheld too, even though
/// neither is rendered by the review UI at all pre- or post-answer - they're
/// authoring fields, not review-flow fields. `CardReveal` (below) carries
/// them anyway, purely so an admin editing a card mid-review has a correct,
/// non-blank baseline to save over.
#[derive(Serialize, ToSchema)]
pub struct CardPrompt {
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
    /// `sentence` and `target` itself are withheld; see `CardReveal`.
    pub sentence_before: String,
    /// The text after the blank. See `sentence_before`.
    pub sentence_after: String,
    pub sentence_translation: String,
    pub speech_level: Option<String>,
    pub tense: Option<String>,
    pub grammar_pattern: Option<String>,
    /// Hanja characters for the pre-answer hint span. The reading and each
    /// hint's gloss give the answer away - see `CardReveal::hanja_eum` and
    /// `HanjaHint::trans_word`.
    pub hanja: Option<String>,
    pub hanja_hint_words: Vec<String>,
}

/// Disclosed only once `POST /api/cards/{id}/check` has graded an attempt.
/// Every field here would give the answer away if it shipped any earlier.
#[derive(Serialize, ToSchema)]
pub struct CardReveal {
    pub word: String,
    pub definition: Option<String>,
    pub sentence: String,
    pub target: String,
    pub alternatives: Vec<String>,
    pub hanja_eum: Option<String>,
    pub hanja_hints: Vec<HanjaHint>,
    /// The grammar pattern's possible conjugation endings - a property of
    /// the referenced `grammar_patterns` row, not of this card, but exactly
    /// as spoiling as `target` for any card that uses the pattern, so it
    /// travels with the reveal rather than in the pattern's public
    /// label/tooltip (see `list_enum_lookups`, which admin/authoring
    /// surfaces still fetch endings from - that's a legitimately public use,
    /// picking a pattern rather than guessing one card's answer).
    pub grammar_pattern_endings: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct CheckRequest {
    pub answer: String,
}

#[derive(Serialize, ToSchema)]
pub struct CheckResponse {
    pub correct: bool,
    #[serde(flatten)]
    pub reveal: CardReveal,
}

/// The canonical full-card shape: a `cards` row joined with its English
/// translation and primary example sentence. Shared verbatim by admin
/// search (`admin::search_cards`) and edit (`admin::edit_card`) - previously
/// two independently hand-declared structs that happened to agree on 14 of
/// their fields, which is exactly the kind of duplication that drifts
/// silently over time. Not used by the review flow, which only ever needs
/// the `CardPrompt`/`CardReveal` split above - admin editing isn't gated by
/// the same secrecy concerns, so it gets the whole row upfront.
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

#[derive(Serialize, ToSchema)]
pub struct NextCardResponse {
    #[serde(flatten)]
    prompt: CardPrompt,
    difficulty: Option<f64>,
    guess_count: i64,
    wrong_guess_count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct EnumEntry {
    pub slug: String,
    pub label: String,
    pub tooltip: Option<String>,
    pub rank: Option<i64>,
    pub endings: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct EnumLookups {
    pub pos: Vec<EnumEntry>,
    pub origin_type: Vec<EnumEntry>,
    pub grade: Vec<EnumEntry>,
    pub speech_level: Vec<EnumEntry>,
    pub tense: Vec<EnumEntry>,
    pub grammar_pattern: Vec<EnumEntry>,
}

async fn fetch_enum_entries(
    pool: &SqlitePool,
    table: &str,
    has_tooltip: bool,
    has_rank: bool,
    has_endings: bool,
) -> Result<Vec<EnumEntry>, AppError> {
    let cols = format!(
        "slug, label{}{}{}",
        if has_tooltip { ", tooltip" } else { "" },
        if has_rank { ", rank" } else { "" },
        if has_endings { ", endings" } else { "" },
    );
    let rows = sqlx::query(&format!("SELECT {cols} FROM {table} ORDER BY id"))
        .fetch_all(pool)
        .await?;

    Ok(rows
        .iter()
        .map(|row| EnumEntry {
            slug: row.get("slug"),
            label: row.get("label"),
            tooltip: if has_tooltip { row.get("tooltip") } else { None },
            rank: if has_rank { row.get("rank") } else { None },
            endings: if has_endings { row.get("endings") } else { None },
        })
        .collect())
}

#[utoipa::path(
    get,
    path = "/api/cards/enum-lookups",
    responses(
        (status = 200, description = "Lookup-table values for dropdowns/enum displays", body = EnumLookups),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn list_enum_lookups(
    State(pool): State<SqlitePool>,
    _auth: crate::auth::AuthUser,
) -> Result<Json<EnumLookups>, AppError> {
    Ok(Json(EnumLookups {
        pos: fetch_enum_entries(&pool, "parts_of_speech", false, false, false).await?,
        origin_type: fetch_enum_entries(&pool, "origin_types", false, false, false).await?,
        grade: fetch_enum_entries(&pool, "grades", false, true, false).await?,
        speech_level: fetch_enum_entries(&pool, "speech_levels", false, false, false).await?,
        tense: fetch_enum_entries(&pool, "tenses", false, false, false).await?,
        grammar_pattern: fetch_enum_entries(&pool, "grammar_patterns", true, false, true).await?,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct NextCardEnvelope {
    pub card: Option<NextCardResponse>,
    pub next_due_at: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SuppressedCard {
    card_id: i64,
    word: String,
    trans_word: String,
    sentence: String,
    sentence_translation: String,
    pos: Option<String>,
    grade: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SuppressedCardsResponse {
    cards: Vec<SuppressedCard>,
}

#[derive(Serialize, ToSchema)]
pub struct ReviewResponse {
    success: bool,
}

#[derive(Serialize, ToSchema)]
pub struct DayHistory {
    pub date: String,
    pub total: i64,
    pub correct: i64,
    // Truncated integer, same computation as the status bar percentage
    pub percentage: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ReviewHistoryResponse {
    pub days: Vec<DayHistory>,
}

#[derive(Serialize, ToSchema)]
pub struct HistorySummary {
    pub total_reviews: i64,
    pub total_cards_reviewed: i64,
    pub cards_learning: i64,
    pub cards_review: i64,
    pub cards_relearning: i64,
    pub cards_unseen: i64,
    pub total_accuracy: f64,
    pub avg_reviews_per_day: f64,
    pub first_review_date: Option<String>,
    pub current_streak: i64,
    pub longest_streak: i64,
}

#[derive(Serialize, ToSchema)]
pub struct StatsResponse {
    new_count: i64,
    due_count: i64,
    reviews_today: i64,
    correct_today: i64,
    percentage: Option<i64>,
    next_due_at: Option<String>,
    new_today_count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct OptimizeFsrsResponse {
    success: bool,
    parameters: Vec<f32>,
    review_count: usize,
}

#[derive(Deserialize, Default, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct NextCardQuery {
    // Optional word_id to exclude from the result (used by client prefetch
    // to skip the card currently being shown to the user).
    exclude: Option<i64>,
}

// Accuracy stats must agree everywhere they are shown (status bar, review
// history chart, summary, breakdowns). These fragments define which
// review_history rows count: a card's first-ever review is recorded with
// state = 'learning' and is excluded from accuracy either way.
const COUNTED_REVIEW_SQL: &str = "state != 'learning'";
const CORRECT_REVIEW_SQL: &str = "rating IN ('good', 'easy')";

/// Start of the user's current logical day: `day_boundary_hour` o'clock local
/// time, today if that moment has passed, otherwise yesterday.
fn logical_today_start(day_boundary_hour: i64) -> chrono::DateTime<Utc> {
    let now_local = Local::now();
    let today_start_naive = if now_local.hour() >= day_boundary_hour as u32 {
        now_local.date_naive().and_hms_opt(day_boundary_hour as u32, 0, 0).unwrap()
    } else {
        (now_local.date_naive() - chrono::Duration::days(1))
            .and_hms_opt(day_boundary_hour as u32, 0, 0)
            .unwrap()
    };
    Local
        .from_local_datetime(&today_start_naive)
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

/// Format a UTC datetime for comparison against SQLite datetime() values.
fn sqlite_datetime(dt: chrono::DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// SQLite datetime modifier that shifts a UTC `reviewed_at` so that date()
/// yields its logical day — the same boundary as `logical_today_start`.
fn logical_day_shift(day_boundary_hour: i64) -> String {
    let utc_offset_minutes = i64::from(Local::now().offset().local_minus_utc()) / 60;
    format!("{:+} minutes", utc_offset_minutes - day_boundary_hour * 60)
}

/// Truncated integer accuracy percentage; None when there are no reviews.
fn accuracy_percentage(correct: i64, total: i64) -> Option<i64> {
    (total > 0).then(|| correct * 100 / total)
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

        // For prefetch requests (exclude param is set), use stricter limit to prevent race condition.
        // When the user is on card N (new card #19/20), the prefetch for N+1 should not return
        // a new card because by the time N+1 is displayed, card N will have been reviewed,
        // pushing the count to 20/20 and making N+1 display as 21/20.
        // For normal requests, use the actual limit.
        let is_prefetch = params.exclude.is_some();
        let threshold = if is_prefetch {
            daily_new_card_limit - 1  // Block at limit-1 for prefetch
        } else {
            daily_new_card_limit  // Block at limit for normal fetch
        };

        new_cards_today >= threshold
    };

    // Get next due card (prioritize due cards by due date, then new cards)
    // Exclude suspended cards via user_card_flags
    // Optionally skip a specific card_id (used for client-side prefetch)
    // When daily new card limit is 0 or reached (including limit-1 buffer), only show cards that have been reviewed before
    let exclude_id = params.exclude.unwrap_or(-1);
    
    let new_card_filter = if new_card_limit_reached {
        // If limit is 0 or reached, only show cards that have been reviewed before (have review history)
        "AND EXISTS (SELECT 1 FROM review_history WHERE card_id = c.id AND user_id = ?)"
    } else {
        ""
    };

    let query = format!(
        r#"
        SELECT
            c.id, c.krdict_id, c.word, c.definition, c.hanja, c.hanja_eum,
            pop.slug as pos, ot.slug as origin_type, g.slug as grade,
            ct.trans_word, ct.trans_dfn,
            s.id as sentence_id, s.text as sentence, s.target,
            st.translation as sentence_translation,
            sl.slug as speech_level, tn.slug as tense,
            gp.slug as grammar_pattern,
            cs.difficulty, cs.last_review, cs.stability
        FROM cards c
        LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        INNER JOIN card_translations ct ON c.id = ct.card_id AND ct.language_tag = 'en'
        INNER JOIN sentences s ON c.id = s.card_id
        LEFT JOIN sentence_translations st ON s.id = st.sentence_id
        LEFT JOIN sentence_inflection_hints sih ON s.id = sih.sentence_id
        LEFT JOIN parts_of_speech pop ON pop.id = c.pos_id
        LEFT JOIN origin_types ot ON ot.id = c.origin_type_id
        LEFT JOIN grades g ON g.id = c.grade_id
        LEFT JOIN speech_levels sl ON sl.id = sih.speech_level_id
        LEFT JOIN tenses tn ON tn.id = sih.tense_id
        LEFT JOIN grammar_patterns gp ON gp.id = c.grammar_pattern_id
        LEFT JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
        {}
        AND (ucf.suspended IS NULL OR ucf.suspended = 0)
        AND c.id != ?
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
        new_card_filter
    );

    let mut query_builder = sqlx::query(&query)
        .bind(user_id)
        .bind(user_id)
        .bind(user_id);
    
    // Add extra bind for the new card filter if limit is reached
    if new_card_limit_reached {
        query_builder = query_builder.bind(user_id);
    }
    
    let row = query_builder
        .bind(exclude_id)
        .fetch_optional(&pool)
        .await?;

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
            AND (ucf.suspended IS NULL OR ucf.suspended = 0)
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
                    AND (ucf.suspended IS NULL OR ucf.suspended = 0)
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
            .map(|dt| dt.with_timezone(&Utc));

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
    let speech_level: Option<String> = row.get("speech_level");
    let tense: Option<String> = row.get("tense");
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
                speech_level,
                tense,
                grammar_pattern,
                hanja,
                hanja_hint_words,
            },
            difficulty,
            guess_count,
            wrong_guess_count,
        }),
        next_due_at: None,
    }))
}

// Check an answer against a card: grade it, record the FSRS review, and
// reveal the card's secret half.
#[utoipa::path(
    post,
    path = "/api/cards/{card_id}/check",
    params(("card_id" = i64, Path, description = "Card ID")),
    request_body = CheckRequest,
    responses(
        (status = 200, description = "Answer graded, FSRS state updated, secret fields revealed", body = CheckResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 404, description = "Card doesn't exist", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn check_answer(
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    auth: crate::auth::AuthUser,
    AppJson(payload): AppJson<CheckRequest>,
) -> Result<Json<CheckResponse>, AppError> {
    let user_id = auth.0;

    // Fetch the secret half of the card fresh, by id - this handler is the
    // only place allowed to know `target` before the client does.
    let row = sqlx::query(
        r#"
        SELECT c.word, c.definition, c.hanja, c.hanja_eum,
               s.id as sentence_id, s.text as sentence, s.target,
               gp.endings as grammar_pattern_endings
        FROM cards c
        INNER JOIN sentences s ON c.id = s.card_id
        LEFT JOIN grammar_patterns gp ON gp.id = c.grammar_pattern_id
        WHERE c.id = ?
        "#,
    )
    .bind(card_id)
    .fetch_optional(&pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let word: String = row.get("word");
    let definition: Option<String> = row.get("definition");
    let hanja: Option<String> = row.get("hanja");
    let hanja_eum: Option<String> = row.get("hanja_eum");
    let sentence_id: i64 = row.get("sentence_id");
    let sentence: String = row.get("sentence");
    let target: String = row.get("target");
    let grammar_pattern_endings: Option<String> = row.get("grammar_pattern_endings");

    let alternatives: Vec<String> = sqlx::query_scalar(
        "SELECT alt_target FROM sentence_alternative_targets WHERE sentence_id = ?"
    )
    .bind(sentence_id)
    .fetch_all(&pool)
    .await?;

    let trimmed = payload.answer.trim();
    let correct = trimmed == target || alternatives.iter().any(|alt| alt == trimmed);

    let hanja_hints = hanja_hints_for(&pool, user_id, card_id, &hanja).await?;

    info!(
        "Checking answer for user_id: {}, card_id: {}, correct: {}",
        user_id, card_id, correct
    );

    // Rating is derived from correctness, not client-supplied - the UI only
    // ever produces 1 (Again) or 3 (Good), same as the `ReviewRequest` this
    // folds in used to receive directly (trusted, since the client alone
    // knew whether the answer was right - no longer true now that grading
    // happens here).
    let (rating, rating_str): (u8, &str) = if correct { (3, "good") } else { (1, "again") };

    // Get existing card state if any
    let card_state_row = sqlx::query(
        "SELECT stability, difficulty, last_review
         FROM card_states
         WHERE user_id = ? AND card_id = ?",
    )
    .bind(user_id)
    .bind(card_id)
    .fetch_optional(&pool)
    .await?;

    // Load user's optimized FSRS parameters, or fall back to defaults
    let params_json: Option<String> = sqlx::query_scalar(
        "SELECT parameters FROM user_fsrs_parameters WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?;

    let default_params = DEFAULT_PARAMETERS;
    let custom_params: Option<Vec<f32>> = params_json
        .and_then(|json| serde_json::from_str(&json).ok());
    let params: &[f32] = custom_params.as_deref().unwrap_or(&default_params);

    let fsrs = FSRS::new(Some(params)).map_err(|e| AppError::Internal(format!("FSRS init error: {:?}", e)))?;

    // Fetch user's desired retention setting
    let desired_retention: f64 = sqlx::query_scalar(
        "SELECT desired_retention FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .unwrap_or(0.9);

    let (memory_state, elapsed_days) = if let Some(ref row) = card_state_row {
        // Existing card - load state if stability and difficulty are not NULL
        let stability: Option<f64> = row.get("stability");
        let difficulty: Option<f64> = row.get("difficulty");
        let last_review: Option<String> = row.get("last_review");

        if let (Some(stability), Some(difficulty), Some(last_review_str)) = (stability, difficulty, last_review) {
            // last_review is normally RFC3339 (written by to_rfc3339()), but after a backup
            // restore it may be in SQLite's datetime('now') format ("YYYY-MM-DD HH:MM:SS").
            let last_review_time = chrono::DateTime::parse_from_rfc3339(&last_review_str)
                .map(|dt| dt.with_timezone(&Utc))
                .or_else(|_| {
                    chrono::NaiveDateTime::parse_from_str(&last_review_str, "%Y-%m-%d %H:%M:%S%.f")
                        .or_else(|_| chrono::NaiveDateTime::parse_from_str(&last_review_str, "%Y-%m-%d %H:%M:%S"))
                        .map(|ndt| chrono::DateTime::from_naive_utc_and_offset(ndt, Utc))
                })
                .map_err(|e| AppError::Internal(format!("Invalid date format: {}", e)))?;

            let now = Utc::now();
            let elapsed_days = (now - last_review_time).num_days().max(0) as u32;

            let state = MemoryState {
                stability: stability as f32,
                difficulty: difficulty as f32,
            };

            (Some(state), elapsed_days)
        } else {
            // Row exists but FSRS state is NULL (suppressed new card) - treat as new
            (None, 0)
        }
    } else {
        // New card
        (None, 0)
    };

    // Get next states from FSRS
    let next_states = fsrs
        .next_states(memory_state, desired_retention as f32, elapsed_days)
        .map_err(|e| AppError::Internal(format!("FSRS error: {:?}", e)))?;

    // Select the appropriate state based on rating
    let scheduled_state = match rating {
        1 => next_states.again,
        2 => next_states.hard,
        3 => next_states.good,
        4 => next_states.easy,
        _ => next_states.good,
    };

    // Calculate scheduled days for tracking
    let scheduled_days = scheduled_state.interval;
    let now = Utc::now();

    // Determine new state based on rating
    let new_state = if memory_state.is_none() {
        "learning"
    } else if rating == 1 {
        "relearning"
    } else {
        "review"
    };

    // Update or insert card state
    sqlx::query(
        r#"
        INSERT INTO card_states (user_id, card_id, stability, difficulty, last_review, state)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, card_id) DO UPDATE SET
            stability = excluded.stability,
            difficulty = excluded.difficulty,
            last_review = excluded.last_review,
            state = excluded.state
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .bind(scheduled_state.memory.stability as f64)
    .bind(scheduled_state.memory.difficulty as f64)
    .bind(now.to_rfc3339())
    .bind(new_state)
    .execute(&pool)
    .await?;

    // Insert into review_history with full FSRS metadata
    sqlx::query(
        r#"
        INSERT INTO review_history (user_id, card_id, rating, scheduled_days, elapsed_days, stability, difficulty, state)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .bind(rating_str)
    .bind(scheduled_days as f64)
    .bind(elapsed_days as f64)
    .bind(scheduled_state.memory.stability as f64)
    .bind(scheduled_state.memory.difficulty as f64)
    .bind(new_state)
    .execute(&pool)
    .await?;

    Ok(Json(CheckResponse {
        correct,
        reveal: CardReveal {
            word,
            definition,
            sentence,
            target,
            alternatives,
            hanja_eum,
            hanja_hints,
            grammar_pattern_endings,
        },
    }))
}

// Get statistics
#[utoipa::path(
    get,
    path = "/api/cards/stats",
    responses(
        (status = 200, description = "Status-bar summary stats", body = StatsResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn get_stats(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<StatsResponse>, AppError> {
    let user_id = auth.0;

    // Get daily_new_card_limit setting (0 = suppress all new cards)
    let daily_new_card_limit: i64 = sqlx::query_scalar(
        "SELECT daily_new_card_limit FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .unwrap_or(20);

    // Get day_boundary_hour from user_settings (default to 4)
    let day_boundary_hour: i64 = sqlx::query_scalar(
        "SELECT day_boundary_hour FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .unwrap_or(4);

    // Start of the user's current logical day, as UTC for database comparison
    let today_start = sqlite_datetime(logical_today_start(day_boundary_hour));

    // Count new cards (cards not in card_states, excluding suspended)
    // If daily_new_card_limit is 0, new count is 0 (suppressed)
    let new_count_query = if daily_new_card_limit == 0 {
        // When new cards are suppressed (limit = 0), report 0 new cards
        r#"
        SELECT 0
        "#
    } else {
        r#"
        SELECT COUNT(*)
        FROM cards c
        LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        LEFT JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
        AND (cs.last_review IS NULL)
        AND (ucf.suspended IS NULL OR ucf.suspended = 0)
        "#
    };

    let new_count: i64 = if daily_new_card_limit == 0 {
        sqlx::query_scalar(new_count_query)
            .fetch_one(&pool)
            .await?
    } else {
        sqlx::query_scalar(new_count_query)
            .bind(user_id)
            .bind(user_id)
            .bind(user_id)
            .fetch_one(&pool)
            .await?
    };

    // Count due cards (existing cards with last_review set, excluding suspended)
    let due_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cards c
        LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        INNER JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
        AND cs.last_review IS NOT NULL
        AND (ucf.suspended IS NULL OR ucf.suspended = 0)
        AND datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days') <= datetime('now')
        "#,
    )
    .bind(user_id)
    .bind(user_id)
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    // Count reviews today (after day_boundary_hour)
    let reviews_today: i64 = sqlx::query_scalar(&format!(
        r#"
        SELECT COUNT(*)
        FROM review_history
        WHERE user_id = ?
        AND {COUNTED_REVIEW_SQL}
        AND datetime(reviewed_at) >= datetime(?)
        "#,
    ))
    .bind(user_id)
    .bind(&today_start)
    .fetch_one(&pool)
    .await?;

    // Count correct reviews today
    let correct_today: i64 = sqlx::query_scalar(&format!(
        r#"
        SELECT COUNT(*)
        FROM review_history
        WHERE user_id = ?
        AND {COUNTED_REVIEW_SQL}
        AND {CORRECT_REVIEW_SQL}
        AND datetime(reviewed_at) >= datetime(?)
        "#,
    ))
    .bind(user_id)
    .bind(&today_start)
    .fetch_one(&pool)
    .await?;

    let percentage = accuracy_percentage(correct_today, reviews_today);

    // Find when the next card becomes due
    let next_due_at: Option<String> = sqlx::query_scalar(
        r#"
        SELECT strftime('%Y-%m-%dT%H:%M:%SZ', MIN(datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days')))
        FROM cards c
        LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        INNER JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
        AND datetime(cs.last_review, '+' || CAST(cs.stability AS TEXT) || ' days') > datetime('now')
        AND (ucf.suspended IS NULL OR ucf.suspended = 0)
        "#,
    )
    .bind(user_id)
    .bind(user_id)
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    // Count how many NEW cards were reviewed today
    let new_today_count: i64 = sqlx::query_scalar(
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
    .bind(&today_start)
    .bind(&today_start)
    .fetch_one(&pool)
    .await?;

    Ok(Json(StatsResponse {
        new_count,
        due_count,
        reviews_today,
        correct_today,
        percentage,
        next_due_at,
        new_today_count,
    }))
}

#[utoipa::path(
    put,
    path = "/api/cards/{card_id}/suppress",
    params(("card_id" = i64, Path, description = "Card ID")),
    responses(
        (status = 200, description = "Card suspended from review rotation", body = ReviewResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn suppress_card(
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    auth: crate::auth::AuthUser,
) -> Result<Json<ReviewResponse>, AppError> {
    let user_id = auth.0;
    info!("Suppressing card for user_id: {}, card_id: {}", user_id, card_id);

    // Insert or update user_card_flags to mark as suspended
    sqlx::query(
        r#"
        INSERT INTO user_card_flags (user_id, card_id, suspended)
        VALUES (?, ?, 1)
        ON CONFLICT(user_id, card_id) DO UPDATE SET
            suspended = 1,
            flagged_at = datetime('now')
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .execute(&pool)
    .await?;

    info!("Card suspended successfully");

    Ok(Json(ReviewResponse { success: true }))
}

// List all suspended cards for the user
#[utoipa::path(
    get,
    path = "/api/cards/suppressed",
    responses(
        (status = 200, description = "All suspended cards for the user", body = SuppressedCardsResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn list_suppressed_cards(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<SuppressedCardsResponse>, AppError> {
    let user_id = auth.0;
    info!("Listing suspended cards for user_id: {}", user_id);

    let rows = sqlx::query(
        r#"
        SELECT
            c.id, c.word, pop.slug as pos, g.slug as grade,
            ct.trans_word,
            s.text as sentence,
            st.translation as sentence_translation
        FROM cards c
        INNER JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        INNER JOIN card_translations ct ON c.id = ct.card_id AND ct.language_tag = 'en'
        INNER JOIN sentences s ON c.id = s.card_id
        LEFT JOIN sentence_translations st ON s.id = st.sentence_id
        LEFT JOIN parts_of_speech pop ON pop.id = c.pos_id
        LEFT JOIN grades g ON g.id = c.grade_id
        WHERE ucf.suspended = 1
        ORDER BY c.word ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let cards: Vec<SuppressedCard> = rows
        .iter()
        .map(|row| SuppressedCard {
            card_id: row.get("id"),
            word: row.get("word"),
            trans_word: row.get("trans_word"),
            sentence: row.get("sentence"),
            sentence_translation: row.get("sentence_translation"),
            pos: row.get("pos"),
            grade: row.get("grade"),
        })
        .collect();

    Ok(Json(SuppressedCardsResponse { cards }))
}

#[utoipa::path(
    put,
    path = "/api/cards/{card_id}/unsuppress",
    params(("card_id" = i64, Path, description = "Card ID")),
    responses(
        (status = 200, description = "Card un-suspended", body = ReviewResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn unsuppress_card(
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    auth: crate::auth::AuthUser,
) -> Result<Json<ReviewResponse>, AppError> {
    let user_id = auth.0;
    info!("Unsuspending card for user_id: {}, card_id: {}", user_id, card_id);

    sqlx::query(
        r#"
        UPDATE user_card_flags
        SET suspended = 0
        WHERE user_id = ? AND card_id = ?
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .execute(&pool)
    .await?;

    info!("Card unsuspended successfully");

    Ok(Json(ReviewResponse { success: true }))
}

#[utoipa::path(
    get,
    path = "/api/cards/history",
    responses(
        (status = 200, description = "Per-day review history for a rolling window", body = ReviewHistoryResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn get_review_history(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<ReviewHistoryResponse>, AppError> {
    let user_id = auth.0;

    // Get day_boundary_hour from user_settings (default 4)
    let day_boundary_hour: i64 = sqlx::query_scalar(
        "SELECT day_boundary_hour FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .unwrap_or(4);

    // Same logical-day definition as get_stats, so today's bucket here matches
    // the status bar exactly. The window covers today plus the 4 days before.
    let day_shift = logical_day_shift(day_boundary_hour);
    let window_start = sqlite_datetime(
        logical_today_start(day_boundary_hour) - chrono::Duration::days(4),
    );

    let rows = sqlx::query(&format!(
        r#"
        SELECT
            date(datetime(reviewed_at, ?)) AS day,
            COUNT(*) AS total,
            SUM(CASE WHEN {CORRECT_REVIEW_SQL} THEN 1 ELSE 0 END) AS correct
        FROM review_history
        WHERE user_id = ?
          AND {COUNTED_REVIEW_SQL}
          AND datetime(reviewed_at) >= datetime(?)
        GROUP BY day
        ORDER BY day ASC
        "#,
    ))
    .bind(&day_shift)
    .bind(user_id)
    .bind(&window_start)
    .fetch_all(&pool)
    .await?;

    let days = rows
        .iter()
        .map(|row| {
            let total: i64 = row.get("total");
            let correct: i64 = row.get("correct");
            DayHistory {
                date: row.get("day"),
                total,
                correct,
                percentage: accuracy_percentage(correct, total).unwrap_or(0),
            }
        })
        .collect();

    Ok(Json(ReviewHistoryResponse { days }))
}

#[utoipa::path(
    get,
    path = "/api/cards/history-summary",
    responses(
        (status = 200, description = "Aggregate review history summary + streaks", body = HistorySummary),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn get_history_summary(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<HistorySummary>, AppError> {
    let user_id = auth.0;

    // Get day_boundary_hour from user_settings (default 4)
    let day_boundary_hour: i64 = sqlx::query_scalar(
        "SELECT day_boundary_hour FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .unwrap_or(4);

    // Query 1: Aggregate review stats. Accuracy only counts post-first-exposure
    // reviews (same rule as the status bar); the volume stats count everything.
    let stats_row = sqlx::query(&format!(
        r#"
        SELECT
            COUNT(*) AS total_reviews,
            COUNT(DISTINCT card_id) AS total_cards_reviewed,
            COALESCE(
                CAST(SUM(CASE WHEN {COUNTED_REVIEW_SQL} AND {CORRECT_REVIEW_SQL} THEN 1 ELSE 0 END) AS REAL)
                / NULLIF(SUM(CASE WHEN {COUNTED_REVIEW_SQL} THEN 1 ELSE 0 END), 0) * 100,
                0
            ) AS total_accuracy,
            MIN(reviewed_at) AS first_review_date,
            COUNT(DISTINCT date(reviewed_at)) AS distinct_days
        FROM review_history
        WHERE user_id = ?
        "#,
    ))
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    let total_reviews: i64 = stats_row.get("total_reviews");
    let total_cards_reviewed: i64 = stats_row.get("total_cards_reviewed");
    let total_accuracy: f64 = stats_row.get("total_accuracy");
    let distinct_days: i64 = stats_row.get("distinct_days");
    let avg_reviews_per_day = if distinct_days > 0 {
        total_reviews as f64 / distinct_days as f64
    } else {
        0.0
    };

    // Format first_review_date as YYYY-MM-DD
    let first_review_raw: Option<String> = stats_row.get("first_review_date");
    let first_review_date = first_review_raw.and_then(|s| {
        chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.f")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f"))
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S"))
            .ok()
            .map(|dt| dt.format("%Y-%m-%d").to_string())
    });

    // Query 2: Cards by current state
    let state_rows = sqlx::query(
        r#"
        SELECT state, COUNT(*) AS cnt
        FROM card_states
        WHERE user_id = ?
        GROUP BY state
        "#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let mut cards_learning: i64 = 0;
    let mut cards_review: i64 = 0;
    let mut cards_relearning: i64 = 0;
    for row in &state_rows {
        let state: String = row.get("state");
        let cnt: i64 = row.get("cnt");
        match state.as_str() {
            "learning" => cards_learning = cnt,
            "review" => cards_review = cnt,
            "relearning" => cards_relearning = cnt,
            _ => {}
        }
    }

    // Query 2b: Cards never reviewed by this user (same definition as the
    // status bar's new count, but ignoring the daily new card limit)
    let cards_unseen: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cards c
        LEFT JOIN custom_card_metadata ccm ON c.id = ccm.card_id
        LEFT JOIN card_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN user_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
        WHERE (ccm.card_id IS NULL OR ccm.user_id = ?)
        AND (cs.last_review IS NULL)
        AND (ucf.suspended IS NULL OR ucf.suspended = 0)
        "#,
    )
    .bind(user_id)
    .bind(user_id)
    .bind(user_id)
    .fetch_one(&pool)
    .await?;

    // Query 3: All review days (logical days) for streak calculation
    let day_rows = sqlx::query_scalar::<_, String>(
        r#"
        SELECT DISTINCT date(datetime(reviewed_at, ?)) AS day
        FROM review_history
        WHERE user_id = ?
        ORDER BY day ASC
        "#,
    )
    .bind(logical_day_shift(day_boundary_hour))
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    // Compute streaks in Rust
    let dates: Vec<chrono::NaiveDate> = day_rows
        .iter()
        .filter_map(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect();

    // Compute today in the user's boundary-adjusted timezone
    let now_local = Local::now();
    let today_boundary = if now_local.hour() as i64 >= day_boundary_hour {
        now_local.date_naive()
    } else {
        now_local.date_naive() - chrono::Days::new(1)
    };

    let current_streak = if dates.last() == Some(&today_boundary) {
        let mut streak = 1i64;
        for i in (0..dates.len() - 1).rev() {
            if dates[i + 1] - dates[i] == chrono::Duration::days(1) {
                streak += 1;
            } else {
                break;
            }
        }
        streak
    } else {
        0
    };

    let longest_streak = if dates.is_empty() {
        0
    } else {
        let mut max_streak = 1i64;
        let mut current = 1i64;
        for i in 1..dates.len() {
            if dates[i] - dates[i - 1] == chrono::Duration::days(1) {
                current += 1;
            } else {
                max_streak = max_streak.max(current);
                current = 1;
            }
        }
        max_streak.max(current)
    };

    Ok(Json(HistorySummary {
        total_reviews,
        total_cards_reviewed,
        cards_learning,
        cards_review,
        cards_relearning,
        cards_unseen,
        total_accuracy,
        avg_reviews_per_day,
        first_review_date,
        current_streak,
        longest_streak,
    }))
}

// Optimize FSRS parameters from user's review history
#[utoipa::path(
    post,
    path = "/api/cards/optimize-fsrs",
    responses(
        (status = 200, description = "FSRS parameters optimized from full review history", body = OptimizeFsrsResponse),
        (status = 400, description = "No/insufficient review history to optimize from", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn optimize_fsrs(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<OptimizeFsrsResponse>, AppError> {
    let user_id = auth.0;
    info!("Optimizing FSRS parameters for user_id: {}", user_id);

    // Fetch all review history for this user, ordered by card and time
    let rows = sqlx::query(
        r#"
        SELECT card_id, rating, reviewed_at
        FROM review_history
        WHERE user_id = ?
        ORDER BY card_id, reviewed_at ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    if rows.is_empty() {
        return Err(AppError::BadRequest("No review history found".to_string()));
    }

    // Group by card_id and build FSRSItem list
    let mut items: Vec<FSRSItem> = Vec::new();
    let mut current_card_id: Option<i64> = None;
    let mut current_reviews: Vec<FSRSReview> = Vec::new();
    let mut last_review_time: Option<chrono::DateTime<Utc>> = None;

    for row in &rows {
        let card_id: i64 = row.get("card_id");
        let rating_str: String = row.get("rating");
        let reviewed_at_str: String = row.get("reviewed_at");

        let rating: u32 = match rating_str.as_str() {
            "again" => 1,
            "hard" => 2,
            "good" => 3,
            "easy" => 4,
            _ => continue,
        };

        let reviewed_at = chrono::NaiveDateTime::parse_from_str(&reviewed_at_str, "%Y-%m-%dT%H:%M:%S%.f")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(&reviewed_at_str, "%Y-%m-%d %H:%M:%S%.f"))
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(&reviewed_at_str, "%Y-%m-%d %H:%M:%S"))
            .map(|dt| chrono::DateTime::from_naive_utc_and_offset(dt, Utc))
            .map_err(|e| AppError::Internal(format!("Invalid date format: {}", e)))?;

        if current_card_id != Some(card_id) {
            // Save previous card's reviews
            if !current_reviews.is_empty() {
                items.push(FSRSItem {
                    reviews: current_reviews,
                });
            }
            current_card_id = Some(card_id);
            current_reviews = Vec::new();
            last_review_time = None;
        }

        let delta_t = if let Some(last) = last_review_time {
            (reviewed_at - last).num_days().max(0) as u32
        } else {
            0
        };

        current_reviews.push(FSRSReview { rating, delta_t });
        last_review_time = Some(reviewed_at);
    }

    // Push the last card's reviews
    if !current_reviews.is_empty() {
        items.push(FSRSItem {
            reviews: current_reviews,
        });
    }

    // Filter out items where no review has delta_t > 0 (FSRS requirement)
    items.retain(|item| item.reviews.iter().any(|r| r.delta_t > 0));

    info!("Built {} FSRS training items from reviews", items.len());

    if items.is_empty() {
        return Err(AppError::BadRequest(
            "Not enough review history. Each card needs at least 2 reviews to optimize.".to_string()
        ));
    }

    let review_count = items.iter().map(|item| item.reviews.len()).sum::<usize>();

    // Run the optimizer
    let fsrs = FSRS::new(None)
        .map_err(|e| AppError::Internal(format!("FSRS init error: {:?}", e)))?;

    let input = ComputeParametersInput {
        train_set: items,
        progress: None,
        enable_short_term: true,
        num_relearning_steps: None,
    };

    let parameters = fsrs.compute_parameters(input)
        .map_err(|e| AppError::Internal(format!("FSRS optimization error: {:?}", e)))?;

    // Store the optimized parameters
    let params_json = serde_json::to_string(&parameters)
        .map_err(|e| AppError::Internal(format!("JSON serialization error: {}", e)))?;

    sqlx::query(
        r#"
        INSERT INTO user_fsrs_parameters (user_id, parameters)
        VALUES (?, ?)
        ON CONFLICT(user_id) DO UPDATE SET parameters = excluded.parameters
        "#,
    )
    .bind(user_id)
    .bind(&params_json)
    .execute(&pool)
    .await?;

    info!("FSRS parameters optimized from {} reviews", review_count);

    Ok(Json(OptimizeFsrsResponse {
        success: true,
        parameters,
        review_count,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct BreakdownRow {
    label: String,
    reviews: i64,
    correct: i64,
    accuracy: f64,
}

#[derive(Serialize, ToSchema)]
pub struct HistoryBreakdownResponse {
    by_pos: Vec<BreakdownRow>,
    by_origin: Vec<BreakdownRow>,
}

#[utoipa::path(
    get,
    path = "/api/cards/history-breakdown",
    responses(
        (status = 200, description = "Accuracy broken down by part-of-speech and origin type", body = HistoryBreakdownResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn get_history_breakdown(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<Json<HistoryBreakdownResponse>, AppError> {
    let user_id = auth.0;

    // Breakdown by POS — only include rows where pos is not null/empty
    let pos_rows = sqlx::query(&format!(
        r#"
        SELECT
            pop.slug AS label,
            COUNT(*) AS reviews,
            SUM(CASE WHEN {CORRECT_REVIEW_SQL} THEN 1 ELSE 0 END) AS correct
        FROM review_history rh
        JOIN cards c ON c.id = rh.card_id
        JOIN parts_of_speech pop ON pop.id = c.pos_id
        WHERE rh.user_id = ?
          AND {COUNTED_REVIEW_SQL}
        GROUP BY pop.slug
        ORDER BY reviews DESC
        "#,
    ))
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let by_pos: Vec<BreakdownRow> = pos_rows
        .iter()
        .map(|row| {
            let reviews: i64 = row.get("reviews");
            let correct: i64 = row.get("correct");
            let accuracy = if reviews > 0 {
                (correct as f64 / reviews as f64) * 100.0
            } else {
                0.0
            };
            BreakdownRow {
                label: row.get("label"),
                reviews,
                correct,
                accuracy,
            }
        })
        .collect();

    // Breakdown by origin_type
    let origin_rows = sqlx::query(&format!(
        r#"
        SELECT
            ot.slug AS label,
            COUNT(*) AS reviews,
            SUM(CASE WHEN {CORRECT_REVIEW_SQL} THEN 1 ELSE 0 END) AS correct
        FROM review_history rh
        JOIN cards c ON c.id = rh.card_id
        JOIN origin_types ot ON ot.id = c.origin_type_id
        WHERE rh.user_id = ?
          AND {COUNTED_REVIEW_SQL}
        GROUP BY ot.slug
        ORDER BY reviews DESC
        "#,
    ))
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let by_origin: Vec<BreakdownRow> = origin_rows
        .iter()
        .map(|row| {
            let reviews: i64 = row.get("reviews");
            let correct: i64 = row.get("correct");
            let accuracy = if reviews > 0 {
                (correct as f64 / reviews as f64) * 100.0
            } else {
                0.0
            };
            BreakdownRow {
                label: row.get("label"),
                reviews,
                correct,
                accuracy,
            }
        })
        .collect();

    Ok(Json(HistoryBreakdownResponse { by_pos, by_origin }))
}

// Reset FSRS parameters to defaults
#[utoipa::path(
    delete,
    path = "/api/cards/optimize-fsrs",
    responses(
        (status = 204, description = "FSRS parameters reset to library defaults"),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn reset_fsrs_parameters(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
) -> Result<StatusCode, AppError> {
    let user_id = auth.0;
    info!("Resetting FSRS parameters for user_id: {}", user_id);

    sqlx::query("DELETE FROM user_fsrs_parameters WHERE user_id = ?")
        .bind(user_id)
        .execute(&pool)
        .await?;

    info!("FSRS parameters reset to defaults");

    Ok(StatusCode::NO_CONTENT)
}

