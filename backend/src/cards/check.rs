//! `POST /api/cards/{id}/check` - grades an attempt, advances the card's
//! FSRS state, and reveals the half of the card withheld by
//! [`super::next::CardPrompt`].

use axum::{extract::State, Json};
use chrono::Utc;
use fsrs::{FSRS, MemoryState, DEFAULT_PARAMETERS};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::info;
use utoipa::ToSchema;

use crate::error::{AppError, AppJson, AppPath};

use super::{hanja_hints_for, review_prefs, HanjaHint};
use super::time::parse_flexible_datetime;

#[derive(Deserialize, ToSchema)]
pub struct CheckRequest {
    pub answer: String,
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
/// requesting user's review history (see `hanja_hints_for`), and
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
    let sentence_id: i64 = row.get("sentence_id");
    let sentence: String = row.get("sentence");
    let target: String = row.get("target");
    let grammar_pattern_endings_json: String = row.get("grammar_pattern_endings_json");
    let grammar_pattern_endings: Vec<String> =
        serde_json::from_str(&grammar_pattern_endings_json).unwrap_or_default();

    let alternatives: Vec<String> = sqlx::query_scalar(
        "SELECT alt_target FROM targets_alternatives WHERE sentence_id = ?"
    )
    .bind(sentence_id)
    .fetch_all(&pool)
    .await?;

    let correct = is_correct(&payload.answer, &target, &alternatives);

    let hanja_hints = hanja_hints_for(&pool, user_id, card_id, &hanja).await?;

    let inflections = super::inflections_for(&pool, card_id).await?;

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
         FROM cards_states
         WHERE user_id = ? AND card_id = ?",
    )
    .bind(user_id)
    .bind(card_id)
    .fetch_optional(&pool)
    .await?;

    // Load user's optimized FSRS parameters, or fall back to defaults
    let params_json: Option<String> = sqlx::query_scalar(
        "SELECT parameters FROM users_fsrs_parameters WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?;

    let default_params = DEFAULT_PARAMETERS;
    let custom_params: Option<Vec<f32>> = params_json
        .and_then(|json| serde_json::from_str(&json).ok());
    let params: &[f32] = custom_params.as_deref().unwrap_or(&default_params);

    let fsrs = FSRS::new(Some(params)).map_err(|e| AppError::Internal(format!("FSRS init error: {:?}", e)))?;

    let desired_retention = review_prefs(&pool, user_id).await?.desired_retention;

    let (memory_state, elapsed_days) = if let Some(ref row) = card_state_row {
        // Existing card - load state if stability and difficulty are not NULL
        let stability: Option<f64> = row.get("stability");
        let difficulty: Option<f64> = row.get("difficulty");
        let last_review: Option<String> = row.get("last_review");

        if let (Some(stability), Some(difficulty), Some(last_review_str)) = (stability, difficulty, last_review) {
            let last_review_time = parse_flexible_datetime(&last_review_str)
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
        INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state)
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
            back: CardBack {
                word,
                definition,
                sentence,
                target,
                alternatives,
            },
            hanja_hints,
            grammar_pattern_endings,
            inflections,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_is_correct() {
        assert!(is_correct("먹어요", "먹어요", &[]));
    }

    #[test]
    fn mismatch_is_incorrect() {
        assert!(!is_correct("먹어요", "먹었어요", &[]));
    }

    #[test]
    fn leading_and_trailing_whitespace_is_trimmed() {
        assert!(is_correct("  먹어요  ", "먹어요", &[]));
    }

    #[test]
    fn matches_an_alternative_target() {
        let alts = vec!["먹었어요".to_string(), "드셨어요".to_string()];
        assert!(is_correct("드셨어요", "먹어요", &alts));
    }

    // No separate "near miss" / "empty answer" cases: both just exercise the
    // same `!=` comparison mismatch_is_incorrect already covers, with
    // different data. And no NFD-vs-NFC normalization test either - there's
    // no normalization step to test, so a test here could only ever pin
    // down the *absence* of a feature, not catch a regression in one.

    // --- check_answer (handler-level, in-process SQLite) ---------------------

    use crate::auth::AuthUser;
    use crate::test_support::{test_pool, test_user};

    /// The literal target text for card_id (any seeded sample card, 1-50) -
    /// fetched directly rather than hardcoded, so these tests don't need to
    /// know the seed data's actual Korean content.
    async fn target_for(pool: &SqlitePool, card_id: i64) -> String {
        sqlx::query_scalar(
            "SELECT tg.form FROM sentences s JOIN targets tg ON tg.sentence_id = s.id WHERE s.card_id = ?",
        )
        .bind(card_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn card_state(pool: &SqlitePool, user_id: i64, card_id: i64) -> (Option<f64>, Option<f64>, Option<String>, Option<String>) {
        sqlx::query_as(
            "SELECT stability, difficulty, last_review, state FROM cards_states WHERE user_id = ? AND card_id = ?",
        )
        .bind(user_id)
        .bind(card_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn new_card_graded_correct_becomes_learning() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        let card_id = 1;
        let target = target_for(&pool, card_id).await;

        let response = check_answer(
            State(pool.clone()),
            AppPath(card_id),
            AuthUser(user_id),
            AppJson(CheckRequest { answer: target }),
        )
        .await
        .unwrap();

        assert!(response.0.correct);

        let (stability, difficulty, last_review, state) = card_state(&pool, user_id, card_id).await;
        assert_eq!(state.as_deref(), Some("learning"));
        assert!(stability.is_some());
        assert!(difficulty.is_some());
        assert!(last_review.is_some());

        let review_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM review_history WHERE user_id = ? AND card_id = ?")
            .bind(user_id)
            .bind(card_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(review_count, 1);

        let rating: String = sqlx::query_scalar("SELECT rating FROM review_history WHERE user_id = ? AND card_id = ?")
            .bind(user_id)
            .bind(card_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rating, "good");
    }

    #[tokio::test]
    async fn new_card_graded_wrong_is_incorrect_but_still_learning() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        let card_id = 1;

        let response = check_answer(
            State(pool.clone()),
            AppPath(card_id),
            AuthUser(user_id),
            AppJson(CheckRequest { answer: "definitely wrong".to_string() }),
        )
        .await
        .unwrap();

        assert!(!response.0.correct);

        // A first-ever review is "learning" regardless of correctness - the
        // rating (again vs. good) only changes the FSRS-scheduled state
        // once a memory state already exists.
        let (_, _, _, state) = card_state(&pool, user_id, card_id).await;
        assert_eq!(state.as_deref(), Some("learning"));
    }

    #[tokio::test]
    async fn wrong_answer_after_a_review_becomes_relearning() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        let card_id = 1;
        let target = target_for(&pool, card_id).await;

        // First review: correct, establishes a memory state.
        let _ = check_answer(
            State(pool.clone()),
            AppPath(card_id),
            AuthUser(user_id),
            AppJson(CheckRequest { answer: target }),
        )
        .await
        .unwrap();

        // Second review: wrong, with an existing memory state - should
        // demote to "relearning", not stay "learning".
        let response = check_answer(
            State(pool.clone()),
            AppPath(card_id),
            AuthUser(user_id),
            AppJson(CheckRequest { answer: "definitely wrong".to_string() }),
        )
        .await
        .unwrap();

        assert!(!response.0.correct);

        let (_, _, _, state) = card_state(&pool, user_id, card_id).await;
        assert_eq!(state.as_deref(), Some("relearning"));

        let review_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM review_history WHERE user_id = ? AND card_id = ?")
            .bind(user_id)
            .bind(card_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(review_count, 2);
    }

    #[tokio::test]
    async fn existing_row_with_no_last_review_is_treated_as_a_new_card() {
        // `cards_states.stability`/`difficulty` are `NOT NULL DEFAULT 0` in
        // the schema (only `last_review` is nullable) - so despite
        // `check_answer`'s comment describing this branch as "FSRS state is
        // NULL", the tuple match it guards with can only ever fail via
        // `last_review` being NULL; `stability`/`difficulty` are always
        // `Some`. This simulates that real shape: a `cards_states` row that
        // exists (e.g. pre-created by some other flow) but has never
        // actually been reviewed.
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        let card_id = 1;
        let target = target_for(&pool, card_id).await;

        sqlx::query(
            "INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state) VALUES (?, ?, 0, 0, NULL, 'new')",
        )
        .bind(user_id)
        .bind(card_id)
        .execute(&pool)
        .await
        .unwrap();

        let response = check_answer(
            State(pool.clone()),
            AppPath(card_id),
            AuthUser(user_id),
            AppJson(CheckRequest { answer: target }),
        )
        .await
        .unwrap();

        assert!(response.0.correct);

        let (stability, _, _, state) = card_state(&pool, user_id, card_id).await;
        assert_eq!(state.as_deref(), Some("learning"));
        assert!(stability.is_some());
    }
}
