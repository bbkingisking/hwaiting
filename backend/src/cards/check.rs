//! `POST /api/cards/{id}/check` - grades an attempt, advances the card's
//! FSRS state, and reveals the half of the card withheld by
//! [`super::next::CardPrompt`].

use axum::{Json, extract::State};
use chrono::{Timelike, Utc};
use fsrs::MemoryState;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteConnection, SqlitePool};
use tracing::info;
use utoipa::ToSchema;

use crate::error::{AppError, AppJson, AppPath};

use super::scheduling::{Scheduler, due_at};
use super::time::parse_flexible_datetime;
use super::{HanjaHint, hanja_hints_on_connection};

#[derive(Deserialize, ToSchema)]
pub struct CheckRequest {
    pub answer: String,
    pub attempt_token: String,
}

/// The card fields disclosed once an answer is graded, shared with the
/// admin-editing shape - see `cards::Card`'s doc comment - and with
/// `CardFront` (next.rs) for the withheld-until-graded relationship between
/// the two. Split out from `CardReveal` (below) for the same reason
/// `CardFront` was: so `Card` can flatten this struct instead of
/// hand-declaring the same 6 fields a third time. Every field here would
/// give the answer away if it shipped any earlier than `CardReveal` ships it.
#[derive(Serialize, ToSchema)]
pub struct CardBack {
    pub word: String,
    pub definition: Option<String>,
    pub sentence: String,
    pub target: String,
    pub alternatives: Vec<String>,
}

/// One resolved row from `conjugation_matrix_cards`, joined out to the
/// catalog's `slug` rather than repeating its label/category (those are
/// non-spoiling and already available from
/// `list_field_values(fields=inflection_form)` - see `InflectionFormValue`).
/// Only the conjugated `form` itself gives the answer away.
#[derive(Serialize, ToSchema)]
pub struct CardInflection {
    pub form_slug: String,
    pub form: String,
}

/// Disclosed only once `POST /api/cards/{id}/check` has graded an attempt:
/// `CardBack` plus the fields that are genuinely review-flow-specific
/// rather than properties of the card itself - `hanja_hints` depends on the
/// requesting user's review history (see `hanja_hints_on_connection`), and
/// `grammar_pattern_endings`/`inflections` belong to rows referencing this
/// card rather than the card itself - so none has a place on
/// `CardBack`/`Card`.
#[derive(Serialize, ToSchema)]
pub struct CardReveal {
    #[serde(flatten)]
    pub back: CardBack,
    pub hanja_hints: Vec<HanjaHint>,
    /// The grammar pattern's possible conjugation endings - a property of
    /// the referenced `grammar_patterns` row, not of this card, but exactly
    /// as spoiling as `target` for any card that uses the pattern, so it
    /// travels with the reveal rather than in the pattern's public
    /// label/tooltip (see `list_field_values`, which admin/authoring
    /// surfaces still fetch endings from - that's a legitimately public use,
    /// picking a pattern rather than guessing one card's answer). Empty if
    /// the card has no grammar pattern.
    pub grammar_pattern_endings: Vec<String>,
    /// This card's resolved `conjugation_matrix_cards` rows (empty if the
    /// card hasn't been run through the conjugation generator) - each
    /// `form` is exactly as spoiling as `target`, so like
    /// `grammar_pattern_endings` this only ships with the reveal.
    pub inflections: Vec<CardInflection>,
}

#[derive(Serialize, ToSchema)]
pub struct CheckResponse {
    pub correct: bool,
    #[serde(flatten)]
    pub reveal: CardReveal,
}

/// Whether `answer` grades as correct against `target` (or any of
/// `alternatives`): trimmed exact string match, nothing fuzzier. No
/// Unicode normalization happens here - an NFD-decomposed answer (e.g. from
/// some IMEs/OSes, which can produce a jamo-decomposed Hangul string that
/// renders identically to the NFC form the database stores) will not match
/// an NFC `target` even though a person reading both would call them the
/// same word. Extracted from `check_answer` so this comparison - the actual
/// grading rule - can be tested without a database.
fn is_correct(answer: &str, target: &str, alternatives: &[String]) -> bool {
    let trimmed = answer.trim();
    trimmed == target || alternatives.iter().any(|alt| alt == trimmed)
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
        (status = 400, description = "Invalid request or invalid, forged, wrong-user, or wrong-card attempt token", body = crate::error::ErrorResponse),
        (status = 409, description = "Review state, selected sentence, or card content changed since the card was fetched", body = crate::error::StaleAnswerResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 404, description = "Card doesn't exist", body = crate::error::ErrorResponse),
        (status = 403, description = "Card is suppressed", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "cards"
)]
pub async fn check_answer(
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    auth: crate::auth::AuthUser,
    AppJson(payload): AppJson<CheckRequest>,
) -> Result<Json<CheckResponse>, AppError> {
    let user_id = auth.0;

    // Reserve the writer before reading any grading content. Admin edits
    // update that content and its revision in one transaction, so this read,
    // the revision comparison, and the recorded review share one snapshot.
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    let claims =
        super::attempt::validate_attempt(&mut tx, &payload.attempt_token, user_id, Some(card_id))
            .await?;
    let content = load_answer_content(&mut tx, card_id, claims.sentence_id).await?;
    let correct = is_correct(
        &payload.answer,
        &content.back.target,
        &content.back.alternatives,
    );
    record_review(&mut tx, &claims, &payload.answer, correct).await?;

    // Build the response before committing, so a failed reveal also rolls
    // back the review. Every helper borrows this transaction's connection.
    let reveal = assemble_reveal(&mut tx, user_id, card_id, content).await?;
    tx.commit().await?;

    Ok(Json(CheckResponse { correct, reveal }))
}

/// Content loaded from the validated attempt's snapshot, shared by grading
/// and reveal assembly so the response contains the answer actually graded.
struct AnswerContent {
    back: CardBack,
    hanja: Option<String>,
    grammar_pattern_endings: Vec<String>,
}

async fn load_answer_content(
    connection: &mut SqliteConnection,
    card_id: i64,
    sentence_id: i64,
) -> Result<AnswerContent, AppError> {
    // Fetch the secret half of the card fresh, by id - this handler is the
    // only place allowed to know `target` before the client does.
    //
    // grammar_pattern_endings folds grammar_patterns_endings' rows
    // (migration 20240101000040 decomposed the old `gp.endings` string into
    // one row per literal ending) into a JSON array in seed order - see
    // field_values::fetch_field_values, which does the same thing for
    // grammar_patterns' own public listing.
    let row = sqlx::query(
        r#"
        SELECT c.word, c.definition, c.hanja,
               s.id as sentence_id, s.text as sentence, tg.form as target,
               COALESCE((SELECT json_group_array(ending) FROM
                (SELECT ending FROM grammar_patterns_endings
                 WHERE grammar_pattern_id = tg.grammar_pattern_id ORDER BY id)
               ), '[]') as grammar_pattern_endings_json
        FROM cards c
        INNER JOIN sentences s ON c.id = s.card_id
        INNER JOIN targets tg ON tg.sentence_id = s.id
        WHERE c.id = ? AND s.id = ?
        "#,
    )
    .bind(card_id)
    .bind(sentence_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(AppError::NotFound)?;

    let word: String = row.get("word");
    let definition: Option<String> = row.get("definition");
    let hanja: Option<String> = row.get("hanja");
    let sentence_id: i64 = row.get("sentence_id");
    let sentence: String = row.get("sentence");
    let target: String = row.get("target");
    let grammar_pattern_endings_json: String = row.get("grammar_pattern_endings_json");
    let grammar_pattern_endings: Vec<String> =
        serde_json::from_str(&grammar_pattern_endings_json).unwrap_or_default();

    let alternatives: Vec<String> =
        sqlx::query_scalar("SELECT alt_target FROM targets_alternatives WHERE sentence_id = ?")
            .bind(sentence_id)
            .fetch_all(&mut *connection)
            .await?;

    Ok(AnswerContent {
        back: CardBack {
            word,
            definition,
            sentence,
            target,
            alternatives,
        },
        hanja,
        grammar_pattern_endings,
    })
}

/// Persist the scheduling and history effects of grading, without assembling
/// response fields or fetching hints.
async fn record_review(
    connection: &mut SqliteConnection,
    claims: &super::attempt::AttemptClaims,
    answer: &str,
    correct: bool,
) -> Result<(), AppError> {
    let user_id = claims.user_id;
    let card_id = claims.card_id;
    let sentence_id = claims.sentence_id;
    let revision = claims.review_revision;
    let card_state_row = sqlx::query(
        "SELECT stability, difficulty, last_review, review_revision
         FROM cards_states WHERE user_id = ? AND card_id = ?",
    )
    .bind(user_id)
    .bind(card_id)
    .fetch_optional(&mut *connection)
    .await?;
    info!(
        "Checking answer for user_id: {}, card_id: {}, correct: {}",
        user_id, card_id, correct
    );

    // Rating is derived from correctness, not client-supplied - the UI only
    // ever produces "again" or "good", same as the `ReviewRequest` this
    // folds in used to receive directly (trusted, since the client alone
    // knew whether the answer was right - no longer true now that grading
    // happens here). Binary throughout below, rather than routed through
    // FSRS's 4-way (again/hard/good/easy) rating scale - "hard" and "easy"
    // are never produced by this handler.
    let rating_str = if correct { "good" } else { "again" };

    // Read the model and retention under the same writer reservation as the
    // review: a settings change must not reschedule and then be overwritten
    // by an in-flight answer using the previous policy.
    let scheduler = Scheduler::load(&mut *connection, user_id).await?;
    let now = Utc::now().with_nanosecond(0).unwrap();
    let (memory_state, elapsed_days) = if let Some(ref row) = card_state_row {
        // Existing card - load state if stability and difficulty are not NULL
        let stability: Option<f64> = row.get("stability");
        let difficulty: Option<f64> = row.get("difficulty");
        let last_review: Option<String> = row.get("last_review");

        if let (Some(stability), Some(difficulty), Some(last_review_str)) =
            (stability, difficulty, last_review)
        {
            let last_review_time = parse_flexible_datetime(&last_review_str)
                .map_err(|e| AppError::Internal(format!("Invalid date format: {}", e)))?;

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
    let next_states = scheduler
        .model
        .next_states(memory_state, scheduler.retention, elapsed_days)
        .map_err(|e| AppError::Internal(format!("FSRS error: {:?}", e)))?;

    // This handler only ever grades binary correct/incorrect (see above), so
    // only these two of FSRS's four next-state slots are ever selected.
    let scheduled_state = if correct {
        next_states.good
    } else {
        next_states.again
    };

    // Calculate scheduled days for tracking
    let scheduled_days = scheduled_state.interval;
    let deadline = due_at(now, scheduled_days)?;

    // Determine new state based on rating
    let new_state = if memory_state.is_none() {
        "learning"
    } else if correct {
        "review"
    } else {
        "relearning"
    };

    // cards_states (the scheduling state check_answer's own logic above just
    // computed) and review_history (the record everything else - stats,
    // export, FSRS optimization - reads back) must land together: one
    // transaction, so a failure between the two can't leave them
    // disagreeing about what this review did.
    sqlx::query(
        r#"
        INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, review_revision, due_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, card_id) DO UPDATE SET
            stability = excluded.stability,
            difficulty = excluded.difficulty,
            last_review = excluded.last_review,
            state = excluded.state,
            review_revision = excluded.review_revision,
            due_at = excluded.due_at
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .bind(scheduled_state.memory.stability as f64)
    .bind(scheduled_state.memory.difficulty as f64)
    .bind(now.to_rfc3339())
    .bind(new_state)
    .bind(revision + 1)
    .bind(deadline)
    .execute(&mut *connection)
    .await?;

    // Insert into review_history with full FSRS metadata
    let review_id = sqlx::query(
        r#"
        INSERT INTO review_history (user_id, card_id, sentence_id, rating, scheduled_days, elapsed_days, stability, difficulty, state, reviewed_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(user_id)
    .bind(card_id)
    .bind(sentence_id)
    .bind(rating_str)
    .bind(scheduled_days as f64)
    .bind(elapsed_days as f64)
    .bind(scheduled_state.memory.stability as f64)
    .bind(scheduled_state.memory.difficulty as f64)
    .bind(new_state)
    .bind(super::time::sqlite_datetime(now))
    .execute(&mut *connection)
    .await?
    .last_insert_rowid();

    if !correct {
        sqlx::query(
            "INSERT INTO wrong_answers (review_id, target_sentence_id, answer) VALUES (?, ?, ?)",
        )
        .bind(review_id)
        .bind(sentence_id)
        .bind(answer.trim())
        .execute(&mut *connection)
        .await?;
    }

    Ok(())
}

async fn assemble_reveal(
    connection: &mut SqliteConnection,
    user_id: i64,
    card_id: i64,
    content: AnswerContent,
) -> Result<CardReveal, AppError> {
    let AnswerContent {
        back,
        hanja,
        grammar_pattern_endings,
    } = content;
    // Keep hint reads on the writer's connection: acquiring another pool
    // connection here can deadlock with writers waiting for this transaction.
    let eng_id = crate::enum_lookup::eng_language_id(&mut *connection).await?;
    let hanja_hints =
        hanja_hints_on_connection(&mut *connection, eng_id, user_id, card_id, &hanja).await?;

    let inflections = super::inflections_for(&mut *connection, card_id).await?;

    Ok(CardReveal {
        back,
        hanja_hints,
        grammar_pattern_endings,
        inflections,
    })
}

#[cfg(test)]
#[path = "../tests/cards/check.rs"]
mod tests;
