//! `GET /api/cards/queue` - selects the next due/new card and shapes it into
//! the pre-answer [`CardPrompt`], withholding everything [`super::CardReveal`]
//! would give away.

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};
use utoipa::{IntoParams, ToSchema};

use crate::error::{AppError, AppQuery};

use super::ReviewPrefs;
use super::time::{logical_day_start, sqlite_datetime};

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
    /// The particular prompt selected for this review of the card.
    pub sentence_id: i64,
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
/// so it has no place on `CardFront`/`Card`. Served by `GET /api/cards/queue`.
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
pub struct ReviewQueueCard {
    #[serde(flatten)]
    prompt: CardPrompt,
    /// Return unchanged when submitting an answer.
    review_revision: i64,
    content_revision: i64,
    difficulty: Option<f64>,
    guess_count: i64,
    wrong_guess_count: i64,
}

/// A fresh ordered snapshot; fetching does not consume an allowance.
#[derive(Serialize, ToSchema)]
pub struct ReviewQueueResponse {
    pub cards: Vec<ReviewQueueCard>,
    pub next_due_at: Option<String>,
    /// Refresh before promoting buffered cards after this deadline.
    pub refresh_at: String,
}

#[derive(Deserialize, Default, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReviewQueueQuery {
    /// Maximum number of cards. Omit to return the full currently eligible queue.
    /// The daily new-card allowance applies independently of this response limit.
    pub limit: Option<i64>,
}

#[utoipa::path(
    get, path = "/api/cards/queue", params(ReviewQueueQuery),
    responses(
        (status = 200, description = "Ordered review queue snapshot", body = ReviewQueueResponse),
        (status = 400, description = "Invalid batch size", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse)
    ),
    security(("cookie_auth" = [])), tag = "cards"
)]
pub async fn get_review_queue(
    State(pool): State<SqlitePool>,
    auth: crate::auth::AuthUser,
    AppQuery(params): AppQuery<ReviewQueueQuery>,
) -> Result<Json<ReviewQueueResponse>, AppError> {
    if params.limit.is_some_and(|limit| limit <= 0) {
        return Err(AppError::BadRequest("limit must be positive".into()));
    }
    Ok(Json(select_cards(&pool, auth.0, params.limit).await?))
}

async fn select_cards(
    pool: &SqlitePool,
    user_id: i64,
    limit: Option<i64>,
) -> Result<ReviewQueueResponse, AppError> {
    let eng_id = crate::enum_lookup::eng_language_id(pool).await?;
    let mut tx = pool.begin().await?;
    let now = chrono::Local::now();
    let now_epoch = now.timestamp();

    info!("Getting review queue for user_id: {}", user_id);

    let ReviewPrefs {
        day_boundary_hour,
        daily_new_card_limit,
        ..
    } = sqlx::query_as::<_, ReviewPrefs>(
        "SELECT day_boundary_hour, daily_new_card_limit FROM users_settings WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or(ReviewPrefs {
        day_boundary_hour: 4,
        daily_new_card_limit: 20,
    });

    // Start of "today" per day_boundary_hour - same helper get_stats uses.
    let today_start_str = sqlite_datetime(logical_day_start(&now, day_boundary_hour, 0));

    // Count how many NEW cards the user has reviewed today
    // A "new" card is one where it's the user's first review (no prior review_history)
    // Check if new cards are suppressed (limit = 0) or if daily limit is reached
    let mut remaining = 0;
    let new_card_limit_reached = if daily_new_card_limit == 0 {
        true // Suppress all new cards
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
            "#,
        )
        .bind(user_id)
        .bind(&today_start_str)
        .bind(&today_start_str)
        .fetch_one(&mut *tx)
        .await?;

        remaining = (daily_new_card_limit - new_cards_today).max(0);
        remaining == 0
    };

    let query = format!(
        r#"
        WITH candidates AS (SELECT
            c.id, c.krdict_id, c.word, c.definition, c.hanja,
            pop.slug as pos, ot.slug as origin_type, g.slug as grade,
            ct.trans_word, ct.trans_dfn,
            s.id as sentence_id, s.text as sentence, tg.form as target,
            st.translation as sentence_translation,
            sl.slug as speech_level, tn.slug as tense,
            tg.is_honorific, tg.is_humble,
            gp.slug as grammar_pattern,
            c.frequency_rank, cs.due_at,
            EXISTS (SELECT 1 FROM review_history rh WHERE rh.card_id = c.id AND rh.user_id = ?) AS reviewed,
            cs.difficulty, cs.last_review, cs.stability,
            c.content_revision, COALESCE(cs.review_revision, 0) AS review_revision
        FROM cards c
        INNER JOIN cards_translations ct ON c.id = ct.card_id AND ct.language_id = ?
        INNER JOIN sentences s ON s.id = (
            SELECT s2.id FROM sentences s2
            INNER JOIN targets t2 ON t2.sentence_id = s2.id
            WHERE s2.card_id = c.id
            ORDER BY COALESCE((
                SELECT MAX(rh.id) FROM review_history rh
                WHERE rh.user_id = ? AND rh.sentence_id = s2.id
            ), 0), s2.id
            LIMIT 1
        )
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
        AND (
            cs.last_review IS NULL
            OR cs.due_at <= {now_epoch}
        )
        ),
        eligible_new AS (
            SELECT id FROM candidates WHERE reviewed = 0
            ORDER BY frequency_rank ASC NULLS LAST, id LIMIT ?
        )
        SELECT * FROM candidates
        WHERE reviewed = 1 OR id IN (SELECT id FROM eligible_new)
        ORDER BY
            CASE WHEN last_review IS NULL THEN 1 ELSE 0 END,
            due_at ASC, frequency_rank ASC NULLS LAST, id
        LIMIT ?
        "#
    );

    let query_builder = sqlx::query(&query)
        .bind(user_id)
        .bind(eng_id)
        .bind(user_id)
        .bind(eng_id)
        .bind(user_id)
        .bind(user_id);

    let rows = query_builder
        .bind(remaining)
        .bind(limit.unwrap_or(-1)) // SQLite: negative LIMIT means no response limit.
        .fetch_all(&mut *tx)
        .await?;
    let future_due: Option<String> = sqlx::query_scalar(
        "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', MIN(cs.due_at), 'unixepoch')
         FROM cards_states cs
         LEFT JOIN users_card_flags f ON f.card_id = cs.card_id AND f.user_id = cs.user_id
         WHERE cs.user_id = ? AND cs.due_at > ? AND COALESCE(f.suppressed, 0) = 0",
    )
    .bind(user_id)
    .bind(now_epoch)
    .fetch_one(&mut *tx)
    .await?;
    let boundary = logical_day_start(&now, day_boundary_hour, 1)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let refresh_at = future_due
        .map(|due| due.min(boundary.clone()))
        .unwrap_or(boundary);

    if rows.is_empty() {
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
            SELECT strftime('%Y-%m-%dT%H:%M:%SZ', MIN(cs.due_at), 'unixepoch')
            FROM cards c
            INNER JOIN cards_states cs ON cs.card_id = c.id AND cs.user_id = ?
            LEFT JOIN users_card_flags ucf ON ucf.card_id = c.id AND ucf.user_id = ?
            WHERE cs.due_at > ?
            AND (ucf.suppressed IS NULL OR ucf.suppressed = 0)
            "#,
        )
        .bind(user_id)
        .bind(user_id)
        .bind(now_epoch)
        .fetch_one(&mut *tx)
        .await?;

        let new_card_reset = if new_card_limit_reached && daily_new_card_limit > 0 {
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
            .fetch_one(&mut *tx)
            .await?;

            (new_cards_waiting > 0).then(|| logical_day_start(&now, day_boundary_hour, 1))
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

        return Ok(ReviewQueueResponse {
            cards: vec![],
            next_due_at,
            refresh_at,
        });
    };

    let mut cards = Vec::with_capacity(rows.len());
    for row in rows {
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
        .fetch_one(&mut *tx)
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
        let hanja_hints =
            super::hanja_hints_on_connection(&mut tx, eng_id, user_id, card_id, &hanja).await?;
        let hanja_hint_words: Vec<String> = hanja_hints.into_iter().map(|h| h.hanja).collect();

        let (sentence_before, sentence_after) = split_sentence(&sentence, &target);

        cards.push(ReviewQueueCard {
            prompt: CardPrompt {
                front: CardFront {
                    card_id,
                    sentence_id: row.get("sentence_id"),
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
            review_revision: row.get("review_revision"),
            content_revision: row.get("content_revision"),
            difficulty,
            guess_count,
            wrong_guess_count,
        });
    }
    Ok(ReviewQueueResponse {
        cards,
        next_due_at: None,
        refresh_at,
    })
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

    use crate::auth::AuthUser;
    use crate::cards::time::parse_flexible_datetime;
    use crate::test_support::{test_pool, test_user};

    async fn queue(pool: &SqlitePool, user_id: i64, limit: i64) -> ReviewQueueResponse {
        get_review_queue(
            State(pool.clone()),
            AuthUser(user_id),
            AppQuery(ReviewQueueQuery { limit: Some(limit) }),
        )
        .await
        .unwrap()
        .0
    }

    fn ids(queue: &ReviewQueueResponse) -> Vec<i64> {
        queue.cards.iter().map(|c| c.prompt.front.card_id).collect()
    }

    #[tokio::test]
    async fn omitted_limit_returns_all_due_cards_and_allowance_bounded_new_cards() {
        let pool = test_pool().await;
        let user = test_user(&pool).await;
        sqlx::query("INSERT INTO users_settings (user_id, daily_new_card_limit) VALUES (?, 3)")
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO review_history (user_id, card_id, rating, reviewed_at, stability, difficulty, state) SELECT ?, id, 'good', datetime('now', '-10 days'), 10, 1, 'review' FROM cards WHERE id <= 25")
            .bind(user).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at) SELECT ?, id, 10, 1, datetime('now', '-10 days'), 'review', 0 FROM cards WHERE id <= 25")
            .bind(user).execute(&pool).await.unwrap();
        let all = get_review_queue(
            State(pool.clone()),
            AuthUser(user),
            AppQuery(ReviewQueueQuery::default()),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(all.cards.len(), 28);
        assert!(ids(&all)[..25].iter().all(|id| *id <= 25));
        assert_eq!(ids(&queue(&pool, user, 5).await), ids(&all)[..5]);
        assert_eq!(ids(&queue(&pool, user, 100).await), ids(&all));
    }

    #[tokio::test]
    async fn batch_clients_share_the_same_allowance_bounded_set() {
        let pool = test_pool().await;
        let user = test_user(&pool).await;
        sqlx::query("INSERT INTO users_settings (user_id, daily_new_card_limit) VALUES (?, 1)")
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE cards SET frequency_rank = NULL")
            .execute(&pool)
            .await
            .unwrap();
        let (a, b, c) = tokio::join!(
            queue(&pool, user, 1),
            queue(&pool, user, 5),
            queue(&pool, user, 20)
        );
        assert_eq!(ids(&a), vec![1]);
        assert_eq!(ids(&a), ids(&b));
        assert_eq!(ids(&b), ids(&c));
        assert!(a.next_due_at.is_none());
        assert!(chrono::DateTime::parse_from_rfc3339(&a.refresh_at).is_ok());
    }

    #[tokio::test]
    async fn out_of_order_first_reviews_do_not_expand_the_new_card_set() {
        let pool = test_pool().await;
        let user = test_user(&pool).await;
        sqlx::query("INSERT INTO users_settings (user_id, daily_new_card_limit) VALUES (?, 3)")
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE cards SET frequency_rank = NULL")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(ids(&queue(&pool, user, 20).await), vec![1, 2, 3]);
        for (card, expected) in [(2, vec![1, 3]), (1, vec![3]), (3, vec![])] {
            sqlx::query("INSERT INTO review_history (user_id, card_id, rating, reviewed_at, stability, difficulty, state) VALUES (?, ?, 'good', datetime('now'), 10, 1, 'learning')")
                .bind(user).bind(card).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at) VALUES (?, ?, 10, 1, datetime('now'), 'learning', unixepoch('now', '+10 days'))")
                .bind(user).bind(card).execute(&pool).await.unwrap();
            assert_eq!(ids(&queue(&pool, user, 20).await), expected);
        }
        assert!(queue(&pool, user, 5).await.next_due_at.is_some());
        // A due repetition is still eligible after exhausting new cards.
        sqlx::query("UPDATE cards_states SET due_at = unixepoch('now', '-1 minute') WHERE user_id = ? AND card_id = 2")
            .bind(user).execute(&pool).await.unwrap();
        assert_eq!(ids(&queue(&pool, user, 5).await), vec![2]);
    }

    #[tokio::test]
    async fn batch_deadline_tracks_reviews_that_become_due_before_the_day_boundary() {
        let pool = test_pool().await;
        let user = test_user(&pool).await;
        sqlx::query("INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at) VALUES (?, 1, 10, 1, datetime('now'), 'learning', unixepoch('now', '+2 minutes'))")
            .bind(user).execute(&pool).await.unwrap();
        let response = queue(&pool, user, 5).await;
        let due: String = sqlx::query_scalar("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', due_at, 'unixepoch') FROM cards_states WHERE user_id = ? AND card_id = 1")
            .bind(user).fetch_one(&pool).await.unwrap();
        assert!(response.refresh_at <= due);
        assert!(!ids(&response).contains(&1));
        let again = queue(&pool, user, 5).await;
        assert_eq!(
            serde_json::to_value(&response.cards).unwrap(),
            serde_json::to_value(&again.cards).unwrap()
        );
        let prompt = serde_json::to_value(&response.cards[0]).unwrap();
        assert!(prompt.get("word").is_none());
        assert!(prompt.get("target").is_none());
        assert!(prompt.get("sentence_id").is_some());
    }

    #[tokio::test]
    async fn batch_size_is_validated_and_zero_daily_limit_is_empty() {
        let pool = test_pool().await;
        let user = test_user(&pool).await;
        for limit in [-1, 0] {
            assert!(matches!(
                get_review_queue(
                    State(pool.clone()),
                    AuthUser(user),
                    AppQuery(ReviewQueueQuery { limit: Some(limit) })
                )
                .await,
                Err(AppError::BadRequest(_))
            ));
        }
        sqlx::query("INSERT INTO users_settings (user_id, daily_new_card_limit) VALUES (?, 0)")
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        let result = queue(&pool, user, 5).await;
        assert!(result.cards.is_empty());
        assert!(result.next_due_at.is_none());
    }

    /// Marks every seeded card except `keep` as already-reviewed and due far
    /// in the future - used to isolate a single card as the only "new" (or
    /// only "due-later") candidate without hand-seeding all 50.
    async fn bury_every_other_card(pool: &SqlitePool, user_id: i64, keep: i64) {
        sqlx::query(
            r#"
            INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at)
            SELECT ?, id, 1000, 0, datetime('now'), 'review', unixepoch('now', '+1000 days') FROM cards WHERE id != ?
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

        // Card 2 has a persisted deadline in the past;
        // every other card (including card 1) is untouched, i.e. "new".
        sqlx::query(
            "INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at) \
             VALUES (?, 2, 1, 0, datetime('now', '-10 days'), 'review', unixepoch('now', '-9 days'))",
        )
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

        let envelope = queue(&pool, user_id, 1).await;
        let card = envelope.cards.first().expect("a card should be due");
        assert_eq!(card.prompt.front.card_id, 2);
    }

    #[tokio::test]
    async fn suppressed_card_is_never_returned() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;

        // Card 2 is due, but suppressed - it must not come back even though
        // it would otherwise win on priority.
        sqlx::query(
            "INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at) \
             VALUES (?, 2, 1, 0, datetime('now', '-10 days'), 'review', unixepoch('now', '-9 days'))",
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

        let envelope = queue(&pool, user_id, 1).await;
        let card = envelope
            .cards
            .first()
            .expect("some other card should still be available");
        assert_ne!(card.prompt.front.card_id, 2);
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
        let envelope = queue(&pool, user_id, 1).await;
        assert!(envelope.cards.is_empty());
        assert!(
            envelope.next_due_at.is_none(),
            "a zero limit does not reset tomorrow"
        );
    }

    #[tokio::test]
    async fn next_due_at_reports_the_earliest_scheduled_due_date() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;

        // Every card has a cards_states row (so none reads as "new"), and
        // card 1's is due soonest.
        bury_every_other_card(&pool, user_id, 1).await;
        sqlx::query(
            "INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, due_at) \
             VALUES (?, 1, 3, 0, datetime('now'), 'review', unixepoch('now', '+3 days'))",
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

        let envelope = queue(&pool, user_id, 1).await;
        assert!(envelope.cards.is_empty(), "nothing should be due yet");
        assert_eq!(envelope.next_due_at, Some(expected));
    }

    #[tokio::test]
    async fn prompts_return_current_revision_and_due_again_accepts_it() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        bury_every_other_card(&pool, user_id, 1).await;
        let fetch = || {
            get_review_queue(
                State(pool.clone()),
                AuthUser(user_id),
                AppQuery(ReviewQueueQuery::default()),
            )
        };
        let card = fetch().await.unwrap().0.cards.into_iter().next().unwrap();
        assert_eq!(card.review_revision, 0);
        assert_eq!(
            fetch()
                .await
                .unwrap()
                .0
                .cards
                .into_iter()
                .next()
                .unwrap()
                .review_revision,
            0
        );
        let answer: String = sqlx::query_scalar("SELECT tg.form FROM sentences s JOIN targets tg ON tg.sentence_id = s.id WHERE s.card_id = 1")
            .fetch_one(&pool).await.unwrap();
        let _ = super::super::check::check_answer(
            State(pool.clone()),
            crate::error::AppPath(1),
            AuthUser(user_id),
            crate::error::AppJson(super::super::check::CheckRequest {
                answer: answer.clone(),
                sentence_id: 1,
                review_revision: 0,
                content_revision: 0,
            }),
        )
        .await
        .unwrap();
        sqlx::query("UPDATE cards_states SET last_review = '2000-01-01T00:00:00Z', due_at = 0 WHERE user_id = ? AND card_id = 1")
            .bind(user_id).execute(&pool).await.unwrap();
        let card = fetch().await.unwrap().0.cards.into_iter().next().unwrap();
        assert_eq!(card.review_revision, 1);
        let _ = super::super::check::check_answer(
            State(pool.clone()),
            crate::error::AppPath(1),
            AuthUser(user_id),
            crate::error::AppJson(super::super::check::CheckRequest {
                answer,
                sentence_id: card.prompt.front.sentence_id,
                review_revision: card.review_revision,
                content_revision: card.content_revision,
            }),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn multiple_sentences_rotate_with_one_card_schedule() {
        let pool = test_pool().await;
        let user_id = test_user(&pool).await;
        bury_every_other_card(&pool, user_id, 1).await;
        let second_id =
            sqlx::query("INSERT INTO sentences (card_id, text) VALUES (1, '새 답입니다.')")
                .execute(&pool)
                .await
                .unwrap()
                .last_insert_rowid();
        sqlx::query("INSERT INTO targets (sentence_id, form) VALUES (?, '답')")
            .bind(second_id)
            .execute(&pool)
            .await
            .unwrap();
        let first_id: i64 = sqlx::query_scalar("SELECT MIN(id) FROM sentences WHERE card_id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        let first_answer: String =
            sqlx::query_scalar("SELECT form FROM targets WHERE sentence_id = ?")
                .bind(first_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let fetch = || {
            get_review_queue(
                State(pool.clone()),
                AuthUser(user_id),
                AppQuery(ReviewQueueQuery::default()),
            )
        };

        for (expected_id, answer, revision) in [
            (first_id, first_answer.as_str(), 0),
            (second_id, "답", 1),
            (first_id, first_answer.as_str(), 2),
        ] {
            let card = fetch().await.unwrap().0.cards.into_iter().next().unwrap();
            assert_eq!(card.prompt.front.card_id, 1);
            assert_eq!(card.prompt.front.sentence_id, expected_id);
            assert_eq!(card.review_revision, revision);
            let result = super::super::check::check_answer(
                State(pool.clone()),
                crate::error::AppPath(1),
                AuthUser(user_id),
                crate::error::AppJson(super::super::check::CheckRequest {
                    answer: answer.into(),
                    sentence_id: expected_id,
                    review_revision: revision,
                    content_revision: 0,
                }),
            )
            .await
            .unwrap();
            assert!(result.0.correct);
            sqlx::query("UPDATE cards_states SET due_at = 0 WHERE user_id = ? AND card_id = 1")
                .bind(user_id)
                .execute(&pool)
                .await
                .unwrap();
        }
        let recorded: Vec<i64> = sqlx::query_scalar(
            "SELECT sentence_id FROM review_history WHERE user_id = ? AND card_id = 1 ORDER BY id",
        )
        .bind(user_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(recorded, vec![first_id, second_id, first_id]);
    }
}
