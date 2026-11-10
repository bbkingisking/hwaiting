use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};
use tracing::{debug, info};
use utoipa::{IntoParams, ToSchema};

use crate::auth::AdminUser;
use crate::cards::{Card, CardBack, CardFront, CardInflection};
use crate::error::{AppError, AppJson, AppPath, AppQuery};

/// Distinguishes "key absent" (`None`, don't touch the column) from "key
/// present" (`Some(v)`), where `v` itself distinguishes explicit `null`
/// (`None`, clear the column) from a value (`Some(String)`). OpenAPI has no
/// way to express this three-state shape, so the generated schema types
/// these fields as plain nullable `Option<String>` — accurate for what a
/// client sends, just not for the absent/null distinction, which is
/// call-shape rather than data-shape.
fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListUsersQuery {
    /// Exact username match. Omit to list every user.
    pub username: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct AdminUserSummary {
    pub id: i64,
    /// `None` for an account created via passkey, which has no username.
    pub username: Option<String>,
    pub is_admin: bool,
    pub created_at: String,
}

#[derive(Serialize, ToSchema)]
pub struct ListUsersResponse {
    pub users: Vec<AdminUserSummary>,
}

#[utoipa::path(
    get,
    path = "/api/admin/users",
    params(ListUsersQuery),
    responses(
        (status = 200, description = "All users, or the one matching ?username= exactly", body = ListUsersResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 403, description = "Valid JWT but not an admin", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "admin"
)]
pub async fn list_users(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    AppQuery(params): AppQuery<ListUsersQuery>,
) -> Result<Json<ListUsersResponse>, AppError> {
    info!("Listing users (username filter: {:?})", params.username);

    // No pagination: this table is small enough that a hard LIMIT or offset
    // scheme would be speculative complexity, not a fix for anything
    // actually happening.
    let rows = match &params.username {
        Some(username) => {
            sqlx::query("SELECT id, username, is_admin, created_at FROM users WHERE username = ?")
                .bind(username)
                .fetch_all(&pool)
                .await?
        }
        None => {
            sqlx::query("SELECT id, username, is_admin, created_at FROM users ORDER BY id ASC")
                .fetch_all(&pool)
                .await?
        }
    };

    let users = rows
        .into_iter()
        .map(|row| AdminUserSummary {
            id: row.get("id"),
            username: row.get("username"),
            is_admin: row.get("is_admin"),
            created_at: row.get("created_at"),
        })
        .collect();

    Ok(Json(ListUsersResponse { users }))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchCardsQuery {
    pub q: String,
}

#[derive(Serialize, ToSchema)]
pub struct SearchCardsResponse {
    pub cards: Vec<Card>,
}

// Search and edit responses must project the same joined card data. The
// latter selects one sentence after the write, so the editor receives the
// exact canonical row it changed rather than reconstructing it from inputs.
const CARD_PROJECTION_SQL: &str = r#"
    SELECT
        c.id, c.content_revision, c.krdict_id, c.word, c.definition, c.hanja,
        pop.slug as pos, ot.slug as origin_type, g.slug as grade,
        ct.trans_word, ct.trans_dfn,
        s.id as sentence_id, s.text as sentence, tg.form as target,
        st.translation as sentence_translation,
        sl.slug as speech_level, tn.slug as tense,
        tg.is_honorific, tg.is_humble,
        gp.slug as grammar_pattern
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
"#;

fn project_card(row: SqliteRow, alternatives: Vec<String>) -> Card {
    let sentence: String = row.get("sentence");
    let target: String = row.get("target");
    let (sentence_before, sentence_after) = crate::cards::split_sentence(&sentence, &target);
    Card {
        front: CardFront {
            card_id: row.get("id"),
            content_revision: row.get("content_revision"),
            sentence_id: row.get("sentence_id"),
            krdict_id: row.get("krdict_id"),
            pos: row.get("pos"),
            origin_type: row.get("origin_type"),
            hanja: row.get("hanja"),
            grade: row.get("grade"),
            trans_word: row.get("trans_word"),
            trans_dfn: row.get("trans_dfn"),
            sentence_before,
            sentence_after,
            sentence_translation: row
                .get::<Option<String>, _>("sentence_translation")
                .unwrap_or_default(),
            inflection_hint: crate::inflection_hints::InflectionHint::from_row(&row),
            grammar_pattern: row.get("grammar_pattern"),
        },
        back: CardBack {
            word: row.get("word"),
            definition: row.get("definition"),
            sentence,
            target,
            alternatives,
        },
    }
}

#[utoipa::path(
    get,
    path = "/api/admin/cards/search",
    params(SearchCardsQuery),
    responses(
        (status = 200, description = "Cards matching a substring search over sentence targets, or an exact card id match (capped at 50)", body = SearchCardsResponse),
        (status = 400, description = "Malformed query string", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 403, description = "Valid JWT but not an admin", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "admin"
)]
pub async fn search_cards(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    AppQuery(params): AppQuery<SearchCardsQuery>,
) -> Result<Json<SearchCardsResponse>, AppError> {
    let q = params.q.trim();
    if q.is_empty() {
        return Ok(Json(SearchCardsResponse { cards: Vec::new() }));
    }

    info!("Admin searching cards by target or card id: {}", q);

    let pattern = format!(
        "%{}%",
        q.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let card_id: Option<i64> = q.parse().ok();
    let eng_id = crate::enum_lookup::eng_language_id(&pool).await?;

    let search_sql = format!(
        "{CARD_PROJECTION_SQL} WHERE tg.form LIKE ? ESCAPE '\\' OR c.id = ? \
         ORDER BY length(tg.form) ASC, tg.form ASC LIMIT 50"
    );
    let rows = sqlx::query(&search_sql)
        .bind(eng_id)
        .bind(eng_id)
        .bind(&pattern)
        .bind(card_id)
        .fetch_all(&pool)
        .await?;

    let mut cards = Vec::with_capacity(rows.len());
    for row in rows {
        let sentence_id: i64 = row.get("sentence_id");
        let alternatives: Vec<String> =
            sqlx::query_scalar("SELECT alt_target FROM targets_alternatives WHERE sentence_id = ?")
                .bind(sentence_id)
                .fetch_all(&pool)
                .await?;

        cards.push(project_card(row, alternatives));
    }

    Ok(Json(SearchCardsResponse { cards }))
}

#[derive(Serialize, ToSchema)]
pub struct CardInflectionsResponse {
    pub inflections: Vec<CardInflection>,
}

#[utoipa::path(
    get,
    path = "/api/admin/cards/{card_id}/inflections",
    params(("card_id" = i64, Path, description = "Card ID")),
    responses(
        (status = 200, description = "This card's resolved conjugation-matrix rows (empty if it hasn't been run through the conjugation generator)", body = CardInflectionsResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 403, description = "Valid JWT but not an admin", body = crate::error::ErrorResponse),
        (status = 404, description = "Card doesn't exist", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "admin"
)]
pub async fn get_card_inflections(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
) -> Result<Json<CardInflectionsResponse>, AppError> {
    // Existence check and inflections fetch share one transaction so a
    // concurrent delete of this card can't land between them - two separate
    // pool queries could otherwise see the card exist, have it deleted, then
    // fetch zero inflection rows and return 200 with an empty list instead
    // of the 404 the deletion should now produce.
    let mut tx = pool.begin().await?;

    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cards WHERE id = ?)")
        .bind(card_id)
        .fetch_one(&mut *tx)
        .await?;

    if !exists {
        return Err(AppError::NotFound);
    }

    let inflections = crate::cards::inflections_for(&mut *tx, card_id).await?;
    tx.commit().await?;

    Ok(Json(CardInflectionsResponse { inflections }))
}

/// Partial card edit. Any field left out of the JSON body is untouched;
/// nullable fields (`definition`, `pos`, `origin_type`, `hanja`,
/// `grade`, `trans_dfn`, `speech_level`, `tense`, `grammar_pattern`,
/// `is_honorific`, `is_humble`) can be explicitly set to `null` to clear
/// the column — that's why they're typed `Option<Option<_>>` rather than
/// `Option<_>`, so "omitted" and "explicit null" deserialize differently.
/// Enum-backed fields are sent as slugs, resolved server-side to
/// lookup-table row IDs. `is_honorific`/`is_humble` are declared here
/// directly rather than flattened from a shared write-shape struct, because
/// this struct's fields need to distinguish omitted from explicit-null
/// throughout (see the double-option pattern above).
#[derive(Deserialize, ToSchema, Default)]
pub struct UpdateCardRequest {
    /// Revision of the content the editor opened. Stale edits are rejected.
    pub expected_content_revision: i64,
    /// The sentence variant being edited; it must belong to the card in the path.
    pub sentence_id: i64,
    pub word: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub definition: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub pos: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub origin_type: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub hanja: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub grade: Option<Option<String>>,
    pub trans_word: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub trans_dfn: Option<Option<String>>,
    pub sentence: Option<String>,
    pub sentence_translation: Option<String>,
    pub target: Option<String>,
    pub alternatives: Option<Vec<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub speech_level: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub tense: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub grammar_pattern: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub is_honorific: Option<Option<bool>>,
    #[serde(default, deserialize_with = "double_option")]
    pub is_humble: Option<Option<bool>>,
}

#[derive(Serialize, ToSchema)]
pub struct EditCardResponse {
    pub success: bool,
    pub content_revision: i64,
    pub card: Card,
    #[serde(skip)]
    pub(crate) changed: bool,
}

fn omit_unchanged<T: PartialEq>(requested: &mut Option<T>, current: T) {
    if requested.as_ref() == Some(&current) {
        *requested = None;
    }
}

#[utoipa::path(
    patch,
    path = "/api/admin/cards/{card_id}",
    params(("card_id" = i64, Path, description = "Card ID")),
    request_body = UpdateCardRequest,
    responses(
        (status = 200, description = "Card updated", body = EditCardResponse),
        (status = 400, description = "Target word doesn't appear in the sentence", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 403, description = "Valid JWT but not an admin", body = crate::error::ErrorResponse),
        (status = 409, description = "Card content has changed; reload before editing", body = crate::error::StaleAnswerResponse),
        (status = 404, description = "Card doesn't exist", body = crate::error::ErrorResponse),
    ),
    security(("cookie_auth" = [])),
    tag = "admin"
)]
pub async fn edit_card(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    AppJson(payload): AppJson<UpdateCardRequest>,
) -> Result<Json<EditCardResponse>, AppError> {
    info!("Admin editing card {}", card_id);

    let UpdateCardRequest {
        sentence_id,
        expected_content_revision,
        mut word,
        mut definition,
        mut pos,
        mut origin_type,
        mut hanja,
        mut grade,
        mut trans_word,
        mut trans_dfn,
        mut sentence,
        mut sentence_translation,
        mut target,
        alternatives,
        speech_level: mut speech_level_slug,
        tense: mut tense_slug,
        grammar_pattern: mut grammar_pattern_slug,
        is_honorific,
        is_humble,
    } = payload;

    debug!(
        "Parsed fields: word={:?}, hanja={:?}, definition={:?}",
        word, hanja, definition
    );

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let content_revision: i64 =
        sqlx::query_scalar("SELECT content_revision FROM cards WHERE id = ?")
            .bind(card_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(AppError::NotFound)?;
    if content_revision != expected_content_revision {
        return Err(AppError::StaleContent);
    }

    // The editor names the exact variant it displayed. Never silently edit
    // the first sentence when a card has more than one.
    let owns_sentence: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sentences WHERE id = ? AND card_id = ?)")
            .bind(sentence_id)
            .bind(card_id)
            .fetch_one(&mut *tx)
            .await?;
    if !owns_sentence {
        return Err(AppError::NotFound);
    }

    // Compare inside the writer transaction, before any updates or enum resolution.
    let eng_id = crate::enum_lookup::eng_language_id(&mut *tx).await?;
    let projection_sql = format!("{CARD_PROJECTION_SQL} WHERE c.id = ? AND s.id = ?");
    let current = sqlx::query(&projection_sql)
        .bind(eng_id)
        .bind(eng_id)
        .bind(card_id)
        .bind(sentence_id)
        .fetch_one(&mut *tx)
        .await?;
    let current_alternatives: Vec<String> = sqlx::query_scalar(
        "SELECT alt_target FROM targets_alternatives WHERE sentence_id = ? ORDER BY id",
    )
    .bind(sentence_id)
    .fetch_all(&mut *tx)
    .await?;
    let mut alternatives = alternatives.map(|values| {
        values
            .into_iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
    });
    // Explicit null clears these NOT NULL columns to false.
    let mut is_honorific = is_honorific.map(|value| value.unwrap_or(false));
    let mut is_humble = is_humble.map(|value| value.unwrap_or(false));

    omit_unchanged(&mut word, current.get("word"));
    omit_unchanged(&mut definition, current.get("definition"));
    omit_unchanged(&mut pos, current.get("pos"));
    omit_unchanged(&mut origin_type, current.get("origin_type"));
    omit_unchanged(&mut hanja, current.get("hanja"));
    omit_unchanged(&mut grade, current.get("grade"));
    omit_unchanged(&mut trans_word, current.get("trans_word"));
    omit_unchanged(&mut trans_dfn, current.get("trans_dfn"));
    omit_unchanged(&mut sentence, current.get("sentence"));
    if sentence_translation.as_ref().is_some_and(|value| {
        current
            .get::<Option<String>, _>("sentence_translation")
            .as_ref()
            == Some(value)
    }) {
        sentence_translation = None;
    }
    omit_unchanged(&mut target, current.get("target"));
    omit_unchanged(&mut speech_level_slug, current.get("speech_level"));
    omit_unchanged(&mut tense_slug, current.get("tense"));
    omit_unchanged(&mut grammar_pattern_slug, current.get("grammar_pattern"));
    omit_unchanged(&mut is_honorific, current.get("is_honorific"));
    omit_unchanged(&mut is_humble, current.get("is_humble"));
    omit_unchanged(&mut alternatives, current_alternatives.clone());

    let changed = word.is_some()
        || definition.is_some()
        || pos.is_some()
        || origin_type.is_some()
        || hanja.is_some()
        || grade.is_some()
        || trans_word.is_some()
        || trans_dfn.is_some()
        || sentence.is_some()
        || sentence_translation.is_some()
        || target.is_some()
        || alternatives.is_some()
        || speech_level_slug.is_some()
        || tense_slug.is_some()
        || grammar_pattern_slug.is_some()
        || is_honorific.is_some()
        || is_humble.is_some();
    if !changed {
        let content_revision =
            sqlx::query_scalar("SELECT content_revision FROM cards WHERE id = ?")
                .bind(card_id)
                .fetch_one(&mut *tx)
                .await?;
        let card = project_card(current, current_alternatives);
        tx.commit().await?;
        return Ok(Json(EditCardResponse {
            success: true,
            content_revision,
            card,
            changed: false,
        }));
    }

    // Resolve enum slugs -> lookup table ids (None: field absent; Some(None): clear it;
    // Some(Some(id)): set it). The frontend sends slugs but the columns store FKs.
    let pos_id = crate::enum_lookup::resolve_optional_id(&mut tx, "parts_of_speech", pos).await?;
    let origin_type_id =
        crate::enum_lookup::resolve_optional_id(&mut tx, "origin_types", origin_type).await?;
    let grade_id = crate::enum_lookup::resolve_optional_id(&mut tx, "grades", grade).await?;
    // grammar_pattern_id lives on `targets` as of migration 20240101000039,
    // not on `cards` - resolved here alongside the other enum slugs, but
    // written into the targets SET block below (alongside
    // speech_level_id/tense_id) rather than the cards SET block.
    let grammar_pattern_id =
        crate::enum_lookup::resolve_optional_id(&mut tx, "grammar_patterns", grammar_pattern_slug)
            .await?;

    // Update cards table — build SET clause dynamically so absent fields are untouched
    // and nullable fields can be explicitly set to NULL
    {
        let mut sets: Vec<&str> = Vec::new();
        if word.is_some() {
            sets.push("word = ?")
        }
        if definition.is_some() {
            sets.push("definition = ?")
        }
        if pos_id.is_some() {
            sets.push("pos_id = ?")
        }
        if origin_type_id.is_some() {
            sets.push("origin_type_id = ?")
        }
        if hanja.is_some() {
            sets.push("hanja = ?")
        }
        if grade_id.is_some() {
            sets.push("grade_id = ?")
        }

        if !sets.is_empty() {
            let sql = format!("UPDATE cards SET {} WHERE id = ?", sets.join(", "));
            debug!("Cards update SQL: {}", sql);
            let mut q = sqlx::query(&sql);
            if let Some(ref v) = word {
                q = q.bind(v.as_str())
            }
            if let Some(ref v) = definition {
                q = q.bind(v.as_deref())
            }
            if let Some(v) = pos_id {
                q = q.bind(v)
            }
            if let Some(v) = origin_type_id {
                q = q.bind(v)
            }
            if let Some(ref v) = hanja {
                q = q.bind(v.as_deref())
            }
            if let Some(v) = grade_id {
                q = q.bind(v)
            }
            let result = q.bind(card_id).execute(&mut *tx).await?;
            debug!("Cards update rows_affected: {}", result.rows_affected());
        }
    }

    // Update cards_translations (first English row)
    {
        let mut sets: Vec<&str> = Vec::new();
        if trans_word.is_some() {
            sets.push("trans_word = ?")
        }
        if trans_dfn.is_some() {
            sets.push("trans_dfn = ?")
        }

        if !sets.is_empty() {
            let eng_id = crate::enum_lookup::eng_language_id(&mut *tx).await?;
            let ct_exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM cards_translations WHERE card_id = ? AND language_id = ?)"
            )
            .bind(card_id)
            .bind(eng_id)
            .fetch_one(&mut *tx)
            .await?;

            if ct_exists {
                let sql = format!(
                    "UPDATE cards_translations SET {} WHERE card_id = ? AND language_id = ?",
                    sets.join(", ")
                );
                let mut q = sqlx::query(&sql);
                if let Some(ref v) = trans_word {
                    q = q.bind(v.as_str())
                }
                if let Some(ref v) = trans_dfn {
                    q = q.bind(v.as_deref())
                }
                q.bind(card_id).bind(eng_id).execute(&mut *tx).await?;
            }
        }
    }

    {
        let sid = sentence_id;
        // Validate that target still appears in the sentence once both
        // sides of this edit are applied - this handler grew as a freeform
        // partial update and used not to re-check it. Without this, a typo
        // in either field produces a
        // card that silently renders with no blank (see
        // cards::split_sentence's fallback). text and target live on
        // separate tables (sentences.text / targets.form - see migration
        // 20240101000026), so whichever side isn't being edited has to be
        // read from wherever it actually lives.
        if sentence.is_some() || target.is_some() {
            let effective_sentence = match &sentence {
                Some(v) => v.clone(),
                None => {
                    sqlx::query_scalar("SELECT text FROM sentences WHERE id = ?")
                        .bind(sid)
                        .fetch_one(&mut *tx)
                        .await?
                }
            };
            let effective_target = match &target {
                Some(v) => v.clone(),
                None => {
                    sqlx::query_scalar("SELECT form FROM targets WHERE sentence_id = ?")
                        .bind(sid)
                        .fetch_one(&mut *tx)
                        .await?
                }
            };
            if !effective_sentence.contains(&effective_target) {
                return Err(AppError::BadRequest(
                    "Target word must appear in the sentence".to_string(),
                ));
            }
        }

        // Update sentences.text + sentences_translations
        if let Some(ref v) = sentence {
            sqlx::query("UPDATE sentences SET text = ? WHERE id = ?")
                .bind(v.as_str())
                .bind(sid)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref st) = sentence_translation {
            // sentences_translations widened to one row per (sentence_id,
            // language_id) as of migration 20240101000045 - pin the update
            // to the eng row explicitly, since an unqualified
            // `WHERE sentence_id = ?` would now touch every language's row
            // for this sentence at once.
            let eng_id = crate::enum_lookup::eng_language_id(&mut *tx).await?;
            sqlx::query("UPDATE sentences_translations SET translation = ? WHERE sentence_id = ? AND language_id = ?")
                .bind(st.as_str())
                .bind(sid)
                .bind(eng_id)
                .execute(&mut *tx)
                .await?;
        }

        // Update targets (form / speech_level / tense / grammar_pattern /
        // is_honorific / is_humble). No exists-check/insert branch needed
        // here unlike the old sentence_inflection_hints: a `targets` row is
        // created unconditionally alongside every sentence now (its `form`
        // is NOT NULL), never left absent the way hint rows used to be.
        //
        // Same dynamically-built-SET-clause shape as the cards/
        // cards_translations updates above, rather than up to six separate
        // single-column statements against the same row.
        let speech_level_id =
            crate::enum_lookup::resolve_optional_id(&mut tx, "speech_levels", speech_level_slug)
                .await?;
        let tense_id =
            crate::enum_lookup::resolve_optional_id(&mut tx, "tenses", tense_slug).await?;

        let mut sets: Vec<&str> = Vec::new();
        if target.is_some() {
            sets.push("form = ?")
        }
        if speech_level_id.is_some() {
            sets.push("speech_level_id = ?")
        }
        if tense_id.is_some() {
            sets.push("tense_id = ?")
        }
        if grammar_pattern_id.is_some() {
            sets.push("grammar_pattern_id = ?")
        }
        if is_honorific.is_some() {
            sets.push("is_honorific = ?")
        }
        if is_humble.is_some() {
            sets.push("is_humble = ?")
        }

        if !sets.is_empty() {
            let sql = format!(
                "UPDATE targets SET {} WHERE sentence_id = ?",
                sets.join(", ")
            );
            let mut q = sqlx::query(&sql);
            if let Some(ref v) = target {
                q = q.bind(v.as_str())
            }
            if let Some(v) = speech_level_id {
                q = q.bind(v)
            }
            if let Some(v) = tense_id {
                q = q.bind(v)
            }
            if let Some(v) = grammar_pattern_id {
                q = q.bind(v)
            }
            if let Some(v) = is_honorific {
                q = q.bind(v)
            }
            if let Some(v) = is_humble {
                q = q.bind(v)
            }
            q.bind(sid).execute(&mut *tx).await?;
        }

        // Update alternative targets
        if let Some(ref alts) = alternatives {
            sqlx::query("DELETE FROM targets_alternatives WHERE sentence_id = ?")
                .bind(sid)
                .execute(&mut *tx)
                .await?;

            for alt in alts {
                sqlx::query(
                    "INSERT INTO targets_alternatives (sentence_id, alt_target) VALUES (?, ?)",
                )
                .bind(sid)
                .bind(alt)
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    let content_revision = sqlx::query_scalar("UPDATE cards SET content_revision = content_revision + 1 WHERE id = ? RETURNING content_revision")
        .bind(card_id)
        .fetch_one(&mut *tx)
        .await?;

    let eng_id = crate::enum_lookup::eng_language_id(&mut *tx).await?;
    let projection_sql = format!("{CARD_PROJECTION_SQL} WHERE c.id = ? AND s.id = ?");
    let row = sqlx::query(&projection_sql)
        .bind(eng_id)
        .bind(eng_id)
        .bind(card_id)
        .bind(sentence_id)
        .fetch_one(&mut *tx)
        .await?;
    let alternatives: Vec<String> =
        sqlx::query_scalar("SELECT alt_target FROM targets_alternatives WHERE sentence_id = ?")
            .bind(sentence_id)
            .fetch_all(&mut *tx)
            .await?;
    let card = project_card(row, alternatives);

    tx.commit().await?;

    info!("Card {} updated successfully", card_id);
    Ok(Json(EditCardResponse {
        success: true,
        content_revision,
        card,
        changed: true,
    }))
}

#[cfg(test)]
#[path = "tests/admin.rs"]
mod tests;
