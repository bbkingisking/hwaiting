use super::*;
use sqlx::SqliteConnection;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, ToSchema)]
pub struct AdminTranslation {
    pub word: String,
    pub definition: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct AdminTarget {
    pub form: String,
    pub alternatives: Vec<String>,
    #[serde(flatten)]
    pub hint: crate::inflection_hints::InflectionHint,
    pub grammar_pattern: Option<crate::cards::GrammarPattern>,
}

#[derive(Serialize, ToSchema)]
pub struct AdminSentence {
    pub sentence_id: i64,
    pub text: String,
    pub target: AdminTarget,
    pub translations: BTreeMap<String, String>,
}

/// Complete authoring content, independent of the current user's L1.
#[derive(Serialize, ToSchema)]
pub struct AdminCardDetail {
    pub card_id: i64,
    pub content_revision: i64,
    pub word: String,
    pub definition: Option<String>,
    pub krdict_id: Option<i64>,
    pub hanja: Option<String>,
    pub pos: Option<String>,
    pub origin_type: Option<String>,
    pub grade: Option<String>,
    pub translations: BTreeMap<String, AdminTranslation>,
    pub sentence: AdminSentence,
}

async fn load_admin_card(
    connection: &mut SqliteConnection,
    card_id: i64,
) -> Result<AdminCardDetail, AppError> {
    let row = sqlx::query("SELECT c.*, p.slug AS pos, o.slug AS origin_type, g.slug AS grade FROM cards c LEFT JOIN parts_of_speech p ON p.id = c.pos_id LEFT JOIN origin_types o ON o.id = c.origin_type_id LEFT JOIN grades g ON g.id = c.grade_id WHERE c.id = ?")
        .bind(card_id).fetch_optional(&mut *connection).await?.ok_or(AppError::NotFound)?;
    let translations = sqlx::query("SELECT l.slug, ct.trans_word, ct.trans_dfn FROM cards_translations ct JOIN languages l ON l.id = ct.language_id WHERE ct.card_id = ? ORDER BY l.slug")
        .bind(card_id).fetch_all(&mut *connection).await?.into_iter().map(|r| (r.get("slug"), AdminTranslation { word: r.get("trans_word"), definition: r.get("trans_dfn") })).collect();
    let r = sqlx::query("SELECT s.id, s.text, t.form, sl.slug AS speech_level, tn.slug AS tense, t.is_honorific, t.is_humble, gp.slug AS grammar_pattern, (SELECT label FROM grammar_patterns_labels WHERE grammar_pattern_id = gp.id AND language_id = (SELECT id FROM languages WHERE slug = 'eng')) AS grammar_pattern_name, (SELECT tooltip FROM grammar_patterns_tooltips WHERE grammar_pattern_id = gp.id AND language_id = (SELECT id FROM languages WHERE slug = 'eng')) AS grammar_pattern_explanation FROM sentences s JOIN targets t ON t.sentence_id = s.id LEFT JOIN speech_levels sl ON sl.id = t.speech_level_id LEFT JOIN tenses tn ON tn.id = t.tense_id LEFT JOIN grammar_patterns gp ON gp.id = t.grammar_pattern_id WHERE s.card_id = ?")
        .bind(card_id).fetch_optional(&mut *connection).await?.ok_or(AppError::NotFound)?;
    let sentence_id = r.get("id");
    let alternatives = sqlx::query_scalar(
        "SELECT alt_target FROM targets_alternatives WHERE sentence_id = ? ORDER BY id",
    )
    .bind(sentence_id)
    .fetch_all(&mut *connection)
    .await?;
    let sentence_translations = sqlx::query("SELECT l.slug, st.translation FROM sentences_translations st JOIN languages l ON l.id = st.language_id WHERE st.sentence_id = ? ORDER BY l.slug")
            .bind(sentence_id).fetch_all(&mut *connection).await?.into_iter().map(|r| (r.get("slug"), r.get("translation"))).collect();
    let sentence = AdminSentence {
        sentence_id,
        text: r.get("text"),
        target: AdminTarget {
            form: r.get("form"),
            alternatives,
            hint: crate::inflection_hints::InflectionHint::from_row(&r),
            grammar_pattern: crate::cards::GrammarPattern::from_row(&r),
        },
        translations: sentence_translations,
    };
    Ok(AdminCardDetail {
        card_id,
        content_revision: row.get("content_revision"),
        word: row.get("word"),
        definition: row.get("definition"),
        krdict_id: row.get("krdict_id"),
        hanja: row.get("hanja"),
        pos: row.get("pos"),
        origin_type: row.get("origin_type"),
        grade: row.get("grade"),
        translations,
        sentence,
    })
}

#[utoipa::path(get, path = "/api/admin/cards/{card_id}", params(("card_id" = i64, Path)), responses((status = 200, body = AdminCardDetail), (status = 404, body = crate::error::ErrorResponse)), security(("cookie_auth" = [])), tag = "admin")]
pub async fn get_admin_card(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
) -> Result<Json<AdminCardDetail>, AppError> {
    let mut tx = pool.begin().await?;
    let card = load_admin_card(&mut tx, card_id).await?;
    tx.commit().await?;
    Ok(Json(card))
}

#[derive(Serialize, ToSchema)]
pub struct AdminCardSummary {
    pub card_id: i64,
    pub word: String,
    pub pos: Option<String>,
}
#[derive(Serialize, ToSchema)]
pub struct AdminSearchResponse {
    pub cards: Vec<AdminCardSummary>,
}

#[utoipa::path(get, path = "/api/admin/cards/search", params(SearchCardsQuery), responses((status = 200, body = AdminSearchResponse)), security(("cookie_auth" = [])), tag = "admin")]
pub async fn search_card_summaries(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    AppQuery(params): AppQuery<SearchCardsQuery>,
) -> Result<Json<AdminSearchResponse>, AppError> {
    let q = params.q.trim();
    if q.is_empty() {
        return Ok(Json(AdminSearchResponse { cards: vec![] }));
    }
    let pattern = format!(
        "%{}%",
        q.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let rows = sqlx::query("SELECT c.id, c.word, p.slug AS pos FROM cards c LEFT JOIN parts_of_speech p ON p.id = c.pos_id WHERE c.id = ? OR c.word LIKE ? ESCAPE '\\' OR EXISTS (SELECT 1 FROM sentences s JOIN targets t ON t.sentence_id = s.id WHERE s.card_id = c.id AND t.form LIKE ? ESCAPE '\\') ORDER BY c.word, c.id LIMIT 50")
        .bind(q.parse::<i64>().ok()).bind(&pattern).bind(&pattern).fetch_all(&pool).await?;
    Ok(Json(AdminSearchResponse {
        cards: rows
            .into_iter()
            .map(|r| AdminCardSummary {
                card_id: r.get("id"),
                word: r.get("word"),
                pos: r.get("pos"),
            })
            .collect(),
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct AdminCardChange {
    pub translation_language: String,
    #[serde(flatten)]
    pub fields: UpdateCardRequest,
}
#[derive(Deserialize, ToSchema)]
pub struct SaveAdminCardRequest {
    pub expected_content_revision: i64,
    pub changes: Vec<AdminCardChange>,
}
#[derive(Serialize, ToSchema)]
pub struct SaveAdminCardResponse {
    pub success: bool,
    pub card: AdminCardDetail,
    #[serde(skip)]
    pub(crate) changed: bool,
}

#[utoipa::path(patch, path = "/api/admin/cards/{card_id}", params(("card_id" = i64, Path)), request_body = SaveAdminCardRequest, responses((status = 200, body = SaveAdminCardResponse), (status = 409, body = crate::error::StaleAnswerResponse), (status = 400, body = crate::error::ErrorResponse)), security(("cookie_auth" = [])), tag = "admin")]
pub async fn save_admin_card(
    _admin: AdminUser,
    State(pool): State<SqlitePool>,
    AppPath(card_id): AppPath<i64>,
    AppJson(payload): AppJson<SaveAdminCardRequest>,
) -> Result<Json<SaveAdminCardResponse>, AppError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let revision: i64 = sqlx::query_scalar("SELECT content_revision FROM cards WHERE id = ?")
        .bind(card_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
    if revision != payload.expected_content_revision {
        return Err(AppError::StaleContent);
    }
    // Fields shared by every translation must not silently become last-write-wins.
    let mut shared = BTreeMap::new();
    for change in &payload.changes {
        macro_rules! check_shared {
            ($($field:ident),* $(,)?) => { $(
                if let Some(value) = &change.fields.$field {
                    let value = serde_json::to_value(value).map_err(|e| AppError::Internal(e.to_string()))?;
                    if shared.insert(stringify!($field), value.clone()).is_some_and(|old| old != value) {
                        return Err(AppError::BadRequest(format!("Conflicting values for shared field '{}'", stringify!($field))));
                    }
                }
            )* };
        }
        check_shared!(
            word,
            definition,
            pos,
            origin_type,
            hanja,
            grade,
            sentence,
            target,
            alternatives,
            speech_level,
            tense,
            grammar_pattern,
            is_honorific,
            is_humble
        );
    }
    let mut seen = BTreeSet::new();
    let mut changed = false;
    for mut change in payload.changes {
        if !seen.insert((
            change.fields.sentence_id,
            change.translation_language.clone(),
        )) {
            return Err(AppError::BadRequest(
                "Duplicate sentence/language change".into(),
            ));
        }
        change.fields.expected_content_revision = revision;
        let language =
            crate::translation_language::TranslationLanguage(change.translation_language);
        let response =
            edit_card_in_transaction(&mut tx, card_id, change.fields, &language, false).await?;
        changed |= response.changed;
    }
    // One save is one content revision, regardless of the number of changed views.
    if changed {
        sqlx::query("UPDATE cards SET content_revision = content_revision + 1 WHERE id = ?")
            .bind(card_id)
            .execute(&mut *tx)
            .await?;
    }
    let card = load_admin_card(&mut tx, card_id).await?;
    tx.commit().await?;
    Ok(Json(SaveAdminCardResponse {
        success: true,
        card,
        changed,
    }))
}
