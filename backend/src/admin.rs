use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};
use utoipa::{IntoParams, ToSchema};

use crate::auth::AdminUser;
use crate::cards::{Card, CardBack, CardFront};
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

#[derive(Deserialize, ToSchema)]
pub struct GenerateInvitesRequest {
    /// Defaults to 1 if omitted from the body, or if the body is omitted
    /// entirely (a POST with no `Content-Type` header at all).
    #[serde(default = "default_invite_count")]
    #[schema(default = 1)]
    pub count: usize,
}

fn default_invite_count() -> usize {
    1
}

#[derive(Serialize, ToSchema)]
pub struct GeneratedInvite {
    pub code: String,
}

#[derive(Serialize, ToSchema)]
pub struct GenerateInvitesResponse {
    pub codes: Vec<GeneratedInvite>,
}

#[derive(Serialize, ToSchema)]
pub struct InviteCode {
    pub code: String,
    pub created_at: String,
    pub used_at: Option<String>,
    pub used_by_username: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ListInvitesResponse {
    pub codes: Vec<InviteCode>,
}

#[utoipa::path(
    post,
    path = "/api/admin/invites",
    request_body(content = Option<GenerateInvitesRequest>, description = "Optional; omit the body \
        entirely (or omit `count` within it) to generate 1 code"),
    responses(
        (status = 201, description = "Invite codes generated (capped at 100 per request)", body = GenerateInvitesResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 403, description = "Valid JWT but not an admin", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "admin"
)]
pub async fn generate_invites(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    payload: Option<AppJson<GenerateInvitesRequest>>,
) -> Result<(StatusCode, Json<GenerateInvitesResponse>), AppError> {
    let count = payload.map_or(1, |AppJson(req)| req.count).min(100); // Cap at 100 codes per request
    
    info!("Generating {} invite codes", count);
    
    let mut codes = Vec::new();
    
    for _ in 0..count {
        let code = generate_code();
        
        sqlx::query("INSERT INTO invite_codes (code) VALUES (?)")
            .bind(&code)
            .execute(&pool)
            .await?;
        
        codes.push(GeneratedInvite { code });
    }
    
    info!("Successfully generated {} invite codes", codes.len());
    
    Ok((StatusCode::CREATED, Json(GenerateInvitesResponse { codes })))
}

#[utoipa::path(
    get,
    path = "/api/admin/invites",
    responses(
        (status = 200, description = "All invite codes, used and unused", body = ListInvitesResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 403, description = "Valid JWT but not an admin", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "admin"
)]
pub async fn list_invites(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
) -> Result<Json<ListInvitesResponse>, AppError> {
    info!("Listing all invite codes");
    
    let rows = sqlx::query(
        "SELECT 
            ic.code, 
            ic.created_at, 
            ic.used_at,
            u.username as used_by_username
         FROM invite_codes ic
         LEFT JOIN users u ON ic.used_by_user_id = u.id
         ORDER BY ic.created_at DESC"
    )
    .fetch_all(&pool)
    .await?;
    
    let codes = rows.into_iter().map(|row| {
        InviteCode {
            code: row.get("code"),
            created_at: row.get("created_at"),
            used_at: row.get("used_at"),
            used_by_username: row.get("used_by_username"),
        }
    }).collect();
    
    Ok(Json(ListInvitesResponse { codes }))
}

#[utoipa::path(
    delete,
    path = "/api/admin/invites/{code}",
    params(("code" = String, Path, description = "Invite code")),
    responses(
        (status = 204, description = "Invite code deleted"),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
        (status = 403, description = "Valid JWT but not an admin", body = crate::error::ErrorResponse),
        (status = 404, description = "Code doesn't exist", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "admin"
)]
pub async fn delete_invite(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    AppPath(code): AppPath<String>,
) -> Result<StatusCode, AppError> {
    info!("Deleting invite code: {}", code);

    let result = sqlx::query("DELETE FROM invite_codes WHERE code = ?")
        .bind(&code)
        .execute(&pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    info!("Invite code deleted: {}", code);

    Ok(StatusCode::NO_CONTENT)
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
    pub username: String,
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
    security(("bearer_auth" = [])),
    tag = "admin"
)]
pub async fn list_users(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    AppQuery(params): AppQuery<ListUsersQuery>,
) -> Result<Json<ListUsersResponse>, AppError> {
    info!("Listing users (username filter: {:?})", params.username);

    // No pagination: same call as list_invites makes for the same reason —
    // this table is small enough that a hard LIMIT or offset scheme would be
    // speculative complexity, not a fix for anything actually happening.
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
    security(("bearer_auth" = [])),
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
        q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
    );
    let card_id: Option<i64> = q.parse().ok();

    let rows = sqlx::query(
        r#"
        SELECT
            c.id, c.krdict_id, c.word, c.definition, c.hanja,
            pop.slug as pos, ot.slug as origin_type, g.slug as grade,
            ct.trans_word, ct.trans_dfn,
            s.id as sentence_id, s.text as sentence, tg.form as target,
            st.translation as sentence_translation,
            sl.slug as speech_level, tn.slug as tense,
            tg.is_honorific, tg.is_humble,
            gp.slug as grammar_pattern
        FROM cards c
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
        WHERE tg.form LIKE ? ESCAPE '\' OR c.id = ?
        ORDER BY length(tg.form) ASC, tg.form ASC
        LIMIT 50
        "#,
    )
    .bind(&pattern)
    .bind(card_id)
    .fetch_all(&pool)
    .await?;

    let mut cards = Vec::with_capacity(rows.len());
    for row in rows {
        let sentence_id: i64 = row.get("sentence_id");
        let alternatives: Vec<String> = sqlx::query_scalar(
            "SELECT alt_target FROM target_alternatives WHERE sentence_id = ?",
        )
        .bind(sentence_id)
        .fetch_all(&pool)
        .await?;

        let sentence: String = row.get("sentence");
        let target: String = row.get("target");
        let (sentence_before, sentence_after) = crate::cards::split_sentence(&sentence, &target);

        cards.push(Card {
            front: CardFront {
                card_id: row.get("id"),
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
        });
    }

    Ok(Json(SearchCardsResponse { cards }))
}

/// Partial card edit. Any field left out of the JSON body is untouched;
/// nullable fields (`definition`, `pos`, `origin_type`, `hanja`,
/// `grade`, `trans_dfn`, `speech_level`, `tense`, `grammar_pattern`,
/// `is_honorific`, `is_humble`) can be explicitly set to `null` to clear
/// the column — that's why they're typed `Option<Option<_>>` rather than
/// `Option<_>`, so "omitted" and "explicit null" deserialize differently.
/// Enum-backed fields are sent as slugs, resolved server-side to
/// lookup-table row IDs. `is_honorific`/`is_humble` aren't flattened from
/// `inflection_hints::InflectionHintWrite` the way `custom_cards`' create/
/// update requests are, because that struct's fields don't distinguish
/// omitted from explicit-null the way this struct's do throughout.
#[derive(Deserialize, ToSchema)]
pub struct UpdateCardRequest {
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
        (status = 404, description = "Card doesn't exist", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
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
        word,
        definition,
        pos,
        origin_type,
        hanja,
        grade,
        trans_word,
        trans_dfn,
        sentence,
        sentence_translation,
        target,
        alternatives,
        speech_level: speech_level_slug,
        tense: tense_slug,
        grammar_pattern: grammar_pattern_slug,
        is_honorific,
        is_humble,
    } = payload;

    debug!(
        "Parsed fields: word={:?}, hanja={:?}, definition={:?}",
        word, hanja, definition
    );

    // Verify the card exists
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cards WHERE id = ?)")
        .bind(card_id)
        .fetch_one(&pool)
        .await?;

    if !exists {
        return Err(AppError::NotFound);
    }

    let mut tx = pool.begin().await?;

    // Resolve enum slugs -> lookup table ids (None: field absent; Some(None): clear it;
    // Some(Some(id)): set it). The frontend sends slugs but the columns store FKs.
    let pos_id = crate::enum_lookup::resolve_optional_id(&mut tx, "parts_of_speech", pos).await?;
    let origin_type_id = crate::enum_lookup::resolve_optional_id(&mut tx, "origin_types", origin_type).await?;
    let grade_id = crate::enum_lookup::resolve_optional_id(&mut tx, "grades", grade).await?;
    let grammar_pattern_id = crate::enum_lookup::resolve_optional_id(&mut tx, "grammar_patterns", grammar_pattern_slug).await?;

    // Update cards table — build SET clause dynamically so absent fields are untouched
    // and nullable fields can be explicitly set to NULL
    {
        let mut sets: Vec<&str> = Vec::new();
        if word.is_some()        { sets.push("word = ?") }
        if definition.is_some()  { sets.push("definition = ?") }
        if pos_id.is_some()         { sets.push("pos_id = ?") }
        if origin_type_id.is_some() { sets.push("origin_type_id = ?") }
        if hanja.is_some()       { sets.push("hanja = ?") }
        if grade_id.is_some()       { sets.push("grade_id = ?") }
        if grammar_pattern_id.is_some() { sets.push("grammar_pattern_id = ?") }

        if !sets.is_empty() {
            let sql = format!("UPDATE cards SET {} WHERE id = ?", sets.join(", "));
            debug!("Cards update SQL: {}", sql);
            let mut q = sqlx::query(&sql);
            if let Some(ref v) = word        { q = q.bind(v.as_str()) }
            if let Some(ref v) = definition  { q = q.bind(v.as_deref()) }
            if let Some(v) = pos_id         { q = q.bind(v) }
            if let Some(v) = origin_type_id { q = q.bind(v) }
            if let Some(ref v) = hanja       { q = q.bind(v.as_deref()) }
            if let Some(v) = grade_id       { q = q.bind(v) }
            if let Some(v) = grammar_pattern_id { q = q.bind(v) }
            let result = q.bind(card_id).execute(&mut *tx).await?;
            debug!("Cards update rows_affected: {}", result.rows_affected());
        }
    }

    // Update card_translations (first English row)
    {
        let mut sets: Vec<&str> = Vec::new();
        if trans_word.is_some() { sets.push("trans_word = ?") }
        if trans_dfn.is_some()  { sets.push("trans_dfn = ?") }

        if !sets.is_empty() {
            let ct_exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM card_translations WHERE card_id = ? AND language_tag = 'en')"
            )
            .bind(card_id)
            .fetch_one(&mut *tx)
            .await?;

            if ct_exists {
                let sql = format!(
                    "UPDATE card_translations SET {} WHERE card_id = ? AND language_tag = 'en'",
                    sets.join(", ")
                );
                let mut q = sqlx::query(&sql);
                if let Some(ref v) = trans_word { q = q.bind(v.as_str()) }
                if let Some(ref v) = trans_dfn  { q = q.bind(v.as_deref()) }
                q.bind(card_id).execute(&mut *tx).await?;
            }
        }
    }

    // Resolve this card's sentence once - shared by the sentence-text
    // update, the target/hint update, and the alternatives update below.
    let sentence_id: Option<i64> =
        sqlx::query_scalar("SELECT id FROM sentences WHERE card_id = ? ORDER BY id LIMIT 1")
            .bind(card_id)
            .fetch_optional(&mut *tx)
            .await?;

    if let Some(sid) = sentence_id {
        // Validate that target still appears in the sentence once both
        // sides of this edit are applied - same invariant
        // custom_cards::update_custom_card enforces, missing here because
        // this handler grew as a freeform partial update and never
        // re-checked it. Without this, a typo in either field produces a
        // card that silently renders with no blank (see
        // cards::split_sentence's fallback). text and target live on
        // separate tables (sentences.text / targets.form - see migration
        // 20240101000026), so whichever side isn't being edited has to be
        // read from wherever it actually lives.
        if sentence.is_some() || target.is_some() {
            let effective_sentence = match &sentence {
                Some(v) => v.clone(),
                None => sqlx::query_scalar("SELECT text FROM sentences WHERE id = ?")
                    .bind(sid)
                    .fetch_one(&mut *tx)
                    .await?,
            };
            let effective_target = match &target {
                Some(v) => v.clone(),
                None => sqlx::query_scalar("SELECT form FROM targets WHERE sentence_id = ?")
                    .bind(sid)
                    .fetch_one(&mut *tx)
                    .await?,
            };
            if !effective_sentence.contains(&effective_target) {
                return Err(AppError::BadRequest(
                    "Target word must appear in the sentence".to_string(),
                ));
            }
        }

        // Update sentences.text + sentence_translations
        if let Some(ref v) = sentence {
            sqlx::query("UPDATE sentences SET text = ? WHERE id = ?")
                .bind(v.as_str())
                .bind(sid)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref st) = sentence_translation {
            sqlx::query("UPDATE sentence_translations SET translation = ? WHERE sentence_id = ?")
                .bind(st.as_str())
                .bind(sid)
                .execute(&mut *tx)
                .await?;
        }

        // Update targets (form / speech_level / tense / is_honorific /
        // is_humble). No exists-check/insert branch needed here unlike the
        // old sentence_inflection_hints: a `targets` row is created
        // unconditionally alongside every sentence now (its `form` is
        // NOT NULL), never left absent the way hint rows used to be.
        let speech_level_id = crate::enum_lookup::resolve_optional_id(&mut tx, "speech_levels", speech_level_slug).await?;
        let tense_id = crate::enum_lookup::resolve_optional_id(&mut tx, "tenses", tense_slug).await?;
        if let Some(ref v) = target {
            sqlx::query("UPDATE targets SET form = ? WHERE sentence_id = ?")
                .bind(v.as_str())
                .bind(sid)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(v) = speech_level_id {
            sqlx::query("UPDATE targets SET speech_level_id = ? WHERE sentence_id = ?")
                .bind(v)
                .bind(sid)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(v) = tense_id {
            sqlx::query("UPDATE targets SET tense_id = ? WHERE sentence_id = ?")
                .bind(v)
                .bind(sid)
                .execute(&mut *tx)
                .await?;
        }
        // is_honorific/is_humble are NOT NULL, so an explicit null (v: None)
        // clears to the column's own default (false) rather than being
        // rejected - there's no NULL state on a boolean column for "clear"
        // to mean anything else.
        if let Some(v) = is_honorific {
            sqlx::query("UPDATE targets SET is_honorific = ? WHERE sentence_id = ?")
                .bind(v.unwrap_or(false))
                .bind(sid)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(v) = is_humble {
            sqlx::query("UPDATE targets SET is_humble = ? WHERE sentence_id = ?")
                .bind(v.unwrap_or(false))
                .bind(sid)
                .execute(&mut *tx)
                .await?;
        }

        // Update alternative targets
        if let Some(ref alts) = alternatives {
            sqlx::query("DELETE FROM target_alternatives WHERE sentence_id = ?")
                .bind(sid)
                .execute(&mut *tx)
                .await?;

            for alt in alts {
                let trimmed = alt.trim();
                if !trimmed.is_empty() {
                    sqlx::query(
                        "INSERT INTO target_alternatives (sentence_id, alt_target) VALUES (?, ?)"
                    )
                    .bind(sid)
                    .bind(trimmed)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
    }

    tx.commit().await?;

    info!("Card {} updated successfully", card_id);
    Ok(Json(EditCardResponse { success: true }))
}


fn generate_code() -> String {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::rng();
    
    (0..8)
        .map(|_| {
            let idx = rng.random_range(0..CHARS.len());
            CHARS[idx] as char
        })
        .collect()
}