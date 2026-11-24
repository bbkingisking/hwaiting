mod editor;
pub(crate) use editor::*;

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

/// Missing fields default to None; present fields must contain a non-null value.
fn non_null_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
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
    /// Null for a passkey-only account without a username.
    #[schema(required)]
    pub username: Option<String>,
    pub is_admin: bool,
    /// Account creation time (RFC 3339, UTC).
    #[schema(format = DateTime)]
    pub created_at: String,
}

#[derive(Serialize, ToSchema)]
pub struct ListUsersResponse {
    pub users: Vec<AdminUserSummary>,
}

#[utoipa::path(
    get,
    path = "/api/admin/users",
    description = "List account IDs, usernames, admin status, and creation times for administrators. An optional username filter matches exactly; without it, returns all users.",
    params(ListUsersQuery),
    responses(
        (status = 200, description = "All users, or the one matching ?username= exactly", body = ListUsersResponse),
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
            sqlx::query("SELECT id, username, is_admin, strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at FROM users WHERE username = ?")
                .bind(username)
                .fetch_all(&pool)
                .await?
        }
        None => {
            sqlx::query("SELECT id, username, is_admin, strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at FROM users ORDER BY id ASC")
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

// The localized projection used to compare and apply each scoped edit inside
// the aggregate writer transaction. Public admin reads use AdminCardDetail.
const CARD_PROJECTION_SQL: &str = r#"
    SELECT
        c.id, c.content_revision, c.krdict_id, c.word, c.definition, c.hanja,
        pop.slug as pos, ot.slug as origin_type, g.slug as grade,
        ct.trans_word, ct.trans_dfn,
        s.id as sentence_id, s.text as sentence, tg.form as target,
        st.translation as sentence_translation,
        sl.slug as speech_level, tn.slug as tense,
        tg.is_honorific, tg.is_humble,
        gp.slug as grammar_pattern,
        (SELECT label FROM grammar_patterns_labels WHERE grammar_pattern_id = gp.id AND language_id = (SELECT id FROM languages WHERE slug = 'eng')) AS grammar_pattern_name,
        (SELECT tooltip FROM grammar_patterns_tooltips WHERE grammar_pattern_id = gp.id AND language_id = (SELECT id FROM languages WHERE slug = 'eng')) AS grammar_pattern_explanation
    FROM cards c
    INNER JOIN cards_translations ct ON c.id = ct.card_id AND ct.language_id = ?
    INNER JOIN sentences s ON c.id = s.card_id
    INNER JOIN targets tg ON tg.sentence_id = s.id
    INNER JOIN sentences_translations st ON s.id = st.sentence_id AND st.language_id = ?
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
            sentence_translation: row.get("sentence_translation"),
            inflection_hint: crate::inflection_hints::InflectionHint::from_row(&row),
            grammar_pattern: crate::cards::GrammarPattern::from_row(&row),
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

#[derive(Serialize, ToSchema)]
pub struct CardInflectionsResponse {
    pub inflections: Vec<CardInflection>,
}

#[utoipa::path(
    get,
    path = "/api/admin/cards/{card_id}/inflections",
    description = "Return a card's generated conjugation-matrix rows for administrators. An existing card without generated inflections returns an empty list.",
    params(("card_id" = i64, Path, description = "Card ID")),
    responses(
        (status = 200, description = "This card's resolved conjugation-matrix rows (empty if it hasn't been run through the conjugation generator)", body = CardInflectionsResponse),
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

/// Partial card edit. Omitted fields are unchanged; supplied values replace them.
/// Null clears nullable fields and is rejected for non-nullable fields.
/// Clear alternatives with [] and reset flags with false.
/// Classification and grammar values use slugs.
#[derive(Deserialize, ToSchema, Default)]
pub struct UpdateCardRequest {
    /// Internal revision check; the aggregate HTTP request supplies its revision once.
    #[serde(skip)]
    pub expected_content_revision: i64,
    /// Internal: the card's single sentence, filled in by the editor.
    #[serde(skip)]
    pub sentence_id: i64,
    /// Omitted leaves the current value unchanged; null is rejected.
    #[serde(default, deserialize_with = "non_null_option")]
    #[schema(nullable = false)]
    pub word: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    /// Omitted leaves the current value unchanged; null clears it.
    pub definition: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    /// Omitted leaves the current value unchanged; null clears it.
    pub pos: Option<Option<crate::classifications::PartOfSpeech>>,
    #[serde(default, deserialize_with = "double_option")]
    /// Omitted leaves the current value unchanged; null clears it.
    pub origin_type: Option<Option<crate::classifications::OriginType>>,
    #[serde(default, deserialize_with = "double_option")]
    /// Omitted leaves the current value unchanged; null clears it.
    pub hanja: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    /// Omitted leaves the current value unchanged; null clears it.
    pub grade: Option<Option<crate::classifications::Grade>>,
    /// Omitted leaves the current value unchanged; null is rejected.
    #[serde(default, deserialize_with = "non_null_option")]
    #[schema(nullable = false)]
    pub trans_word: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    /// Omitted leaves the current value unchanged; null clears it.
    pub trans_dfn: Option<Option<String>>,
    /// Omitted leaves the current value unchanged; null is rejected.
    #[serde(default, deserialize_with = "non_null_option")]
    #[schema(nullable = false)]
    pub sentence: Option<String>,
    /// Omitted leaves the current value unchanged; null is rejected.
    #[serde(default, deserialize_with = "non_null_option")]
    #[schema(nullable = false)]
    pub sentence_translation: Option<String>,
    /// Omitted leaves the current value unchanged; null is rejected.
    #[serde(default, deserialize_with = "non_null_option")]
    #[schema(nullable = false)]
    pub target: Option<String>,
    /// Omitted leaves the current value unchanged; null is rejected.
    #[serde(default, deserialize_with = "non_null_option")]
    #[schema(nullable = false)]
    pub alternatives: Option<Vec<String>>,
    #[serde(default, deserialize_with = "double_option")]
    /// Omitted leaves the current value unchanged; null clears it.
    pub speech_level: Option<Option<crate::classifications::SpeechLevel>>,
    #[serde(default, deserialize_with = "double_option")]
    /// Omitted leaves the current value unchanged; null clears it.
    pub tense: Option<Option<crate::classifications::Tense>>,
    #[serde(default, deserialize_with = "double_option")]
    /// Omitted leaves the current value unchanged; null clears it.
    pub grammar_pattern: Option<Option<String>>,
    /// Omitted leaves the current value unchanged; use false to reset. Null is rejected.
    #[serde(default, deserialize_with = "non_null_option")]
    #[schema(nullable = false)]
    pub is_honorific: Option<bool>,
    /// Omitted leaves the current value unchanged; use false to reset. Null is rejected.
    #[serde(default, deserialize_with = "non_null_option")]
    #[schema(nullable = false)]
    pub is_humble: Option<bool>,
}

#[derive(Serialize, ToSchema)]
pub struct EditCardResponse {
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

#[cfg(test)]
pub async fn edit_card(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    AppJson(mut payload): AppJson<UpdateCardRequest>,
    translation_language: crate::translation_language::TranslationLanguage,
) -> Result<Json<EditCardResponse>, AppError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    // Like the real editor, resolve the card's single sentence server-side.
    payload.sentence_id = sqlx::query_scalar("SELECT id FROM sentences WHERE card_id = ?")
        .bind(card_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
    let response =
        edit_card_in_transaction(&mut tx, card_id, payload, &translation_language, true).await?;
    tx.commit().await?;
    Ok(Json(response))
}

async fn edit_card_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    card_id: i64,
    payload: UpdateCardRequest,
    translation_language: &crate::translation_language::TranslationLanguage,
    advance_revision: bool,
) -> Result<EditCardResponse, AppError> {
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
    // These columns have non-blank CHECK constraints; report a 400 that names
    // the field instead of letting the database turn it into a 500.
    for (field, value) in [
        ("word", word.as_deref()),
        ("trans_word", trans_word.as_deref()),
        ("trans_dfn", trans_dfn.as_ref().and_then(|v| v.as_deref())),
        ("sentence", sentence.as_deref()),
        ("sentence_translation", sentence_translation.as_deref()),
        ("target", target.as_deref()),
    ] {
        if value.is_some_and(|v| v.trim().is_empty()) {
            return Err(AppError::BadRequest(format!("{field} must not be blank")));
        }
    }

    debug!(
        "Parsed fields: word={:?}, hanja={:?}, definition={:?}",
        word, hanja, definition
    );

    let content_revision: i64 =
        sqlx::query_scalar("SELECT content_revision FROM cards WHERE id = ?")
            .bind(card_id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(AppError::NotFound)?;
    if content_revision != expected_content_revision {
        return Err(AppError::StaleContent);
    }

    // The supplied sentence must belong to this card.
    let owns_sentence: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sentences WHERE id = ? AND card_id = ?)")
            .bind(sentence_id)
            .bind(card_id)
            .fetch_one(&mut **tx)
            .await?;
    if !owns_sentence {
        return Err(AppError::NotFound);
    }

    // Compare inside the writer transaction, before any updates or enum resolution.
    let language_id = translation_language.id(&mut **tx).await?;
    let projection_sql = format!("{CARD_PROJECTION_SQL} WHERE c.id = ? AND s.id = ?");
    let current = sqlx::query(&projection_sql)
        .bind(language_id)
        .bind(language_id)
        .bind(card_id)
        .bind(sentence_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(AppError::NotFound)?;
    let current_alternatives: Vec<String> = sqlx::query_scalar(
        "SELECT alt_target FROM targets_alternatives WHERE sentence_id = ? ORDER BY id",
    )
    .bind(sentence_id)
    .fetch_all(&mut **tx)
    .await?;
    let mut alternatives = alternatives.map(|values| {
        values
            .into_iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
    });
    let mut is_honorific = is_honorific;
    let mut is_humble = is_humble;

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
                .fetch_one(&mut **tx)
                .await?;
        let card = project_card(current, current_alternatives);
        return Ok(EditCardResponse {
            content_revision,
            card,
            changed: false,
        });
    }

    // Resolve enum slugs -> lookup table ids (None: field absent; Some(None): clear it;
    // Some(Some(id)): set it). The frontend sends slugs but the columns store FKs.
    let pos_id = crate::enum_lookup::resolve_optional_id(tx, "parts_of_speech", pos).await?;
    let origin_type_id =
        crate::enum_lookup::resolve_optional_id(tx, "origin_types", origin_type).await?;
    let grade_id = crate::enum_lookup::resolve_optional_id(tx, "grades", grade).await?;
    // grammar_pattern_id lives on `targets` as of migration 20240101000039,
    // not on `cards` - resolved here alongside the other enum slugs, but
    // written into the targets SET block below (alongside
    // speech_level_id/tense_id) rather than the cards SET block.
    let grammar_pattern_id =
        crate::enum_lookup::resolve_optional_id(tx, "grammar_patterns", grammar_pattern_slug)
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
            let result = q.bind(card_id).execute(&mut **tx).await?;
            debug!("Cards update rows_affected: {}", result.rows_affected());
        }
    }

    // Update the selected word translation
    {
        let mut sets: Vec<&str> = Vec::new();
        if trans_word.is_some() {
            sets.push("trans_word = ?")
        }
        if trans_dfn.is_some() {
            sets.push("trans_dfn = ?")
        }

        if !sets.is_empty() {
            let language_id = translation_language.id(&mut **tx).await?;
            let ct_exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM cards_translations WHERE card_id = ? AND language_id = ?)"
            )
            .bind(card_id)
            .bind(language_id)
            .fetch_one(&mut **tx)
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
                q.bind(card_id).bind(language_id).execute(&mut **tx).await?;
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
                        .fetch_one(&mut **tx)
                        .await?
                }
            };
            let effective_target = match &target {
                Some(v) => v.clone(),
                None => {
                    sqlx::query_scalar("SELECT form FROM targets WHERE sentence_id = ?")
                        .bind(sid)
                        .fetch_one(&mut **tx)
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
                .execute(&mut **tx)
                .await?;
        }
        if let Some(ref st) = sentence_translation {
            // sentences_translations widened to one row per (sentence_id,
            // language_id) as of migration 20240101000045 - pin the update
            // to the selected translation-language row explicitly, since an unqualified
            // `WHERE sentence_id = ?` would now touch every language's row
            // for this sentence at once.
            let language_id = translation_language.id(&mut **tx).await?;
            sqlx::query("UPDATE sentences_translations SET translation = ? WHERE sentence_id = ? AND language_id = ?")
                .bind(st.as_str())
                .bind(sid)
                .bind(language_id)
                .execute(&mut **tx)
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
            crate::enum_lookup::resolve_optional_id(tx, "speech_levels", speech_level_slug).await?;
        let tense_id = crate::enum_lookup::resolve_optional_id(tx, "tenses", tense_slug).await?;

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
            q.bind(sid).execute(&mut **tx).await?;
        }

        // Update alternative targets
        if let Some(ref alts) = alternatives {
            sqlx::query("DELETE FROM targets_alternatives WHERE sentence_id = ?")
                .bind(sid)
                .execute(&mut **tx)
                .await?;

            for alt in alts {
                sqlx::query(
                    "INSERT INTO targets_alternatives (sentence_id, alt_target) VALUES (?, ?)",
                )
                .bind(sid)
                .bind(alt)
                .execute(&mut **tx)
                .await?;
            }
        }
    }

    let content_revision = if advance_revision {
        sqlx::query_scalar("UPDATE cards SET content_revision = content_revision + 1 WHERE id = ? RETURNING content_revision")
            .bind(card_id).fetch_one(&mut **tx).await?
    } else {
        expected_content_revision
    };

    let language_id = translation_language.id(&mut **tx).await?;
    let projection_sql = format!("{CARD_PROJECTION_SQL} WHERE c.id = ? AND s.id = ?");
    let row = sqlx::query(&projection_sql)
        .bind(language_id)
        .bind(language_id)
        .bind(card_id)
        .bind(sentence_id)
        .fetch_one(&mut **tx)
        .await?;
    let alternatives: Vec<String> =
        sqlx::query_scalar("SELECT alt_target FROM targets_alternatives WHERE sentence_id = ?")
            .bind(sentence_id)
            .fetch_all(&mut **tx)
            .await?;
    let card = project_card(row, alternatives);

    info!("Card {} updated successfully", card_id);
    Ok(EditCardResponse {
        content_revision,
        card,
        changed: true,
    })
}

#[cfg(test)]
#[path = "tests/admin.rs"]
mod tests;
