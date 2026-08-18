//! `GET /api/cards/field-values` - lookup-table values (parts of speech,
//! grades, grammar patterns, ...) for populating dropdowns/badges, both in
//! the review UI and in admin/authoring surfaces. `?fields=` narrows the
//! response to a subset of `FieldName`s, so a client that only cares
//! about e.g. grammar patterns isn't forced to also fetch (and pay the
//! query cost for) the other five.

use axum::{extract::State, Json};
use serde::{
    de::{value::StrDeserializer, IntoDeserializer},
    Deserialize, Serialize,
};
use sqlx::{Row, SqlitePool};
use utoipa::{IntoParams, ToSchema};

use crate::error::{AppError, AppQuery};

#[derive(Serialize, ToSchema)]
pub struct FieldValue {
    pub slug: String,
    pub label: String,
    pub tooltip: Option<String>,
    pub rank: Option<i64>,
    pub endings: Option<String>,
}

/// The seven fields `FieldValues` can return values for, and the only legal
/// values for `?fields=`. Wire values are snake_case (`grammar_pattern`,
/// ...), matching the response's own field names - see
/// `deserialize_field_list`.
#[derive(Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldName {
    Pos,
    OriginType,
    Grade,
    SpeechLevel,
    Tense,
    GrammarPattern,
    InflectionForm,
}

/// The catalog of possible inflected forms (dictionary form, 해요체 present,
/// ...) grouped by category - non-spoiling, unlike the actual conjugated
/// forms for one card (see `CardReveal::inflections` in check.rs, which is
/// gated the same way `grammar_pattern_endings` is). Shaped for the review
/// UI to build a table directly: group by `category_slug` in `sort_order`,
/// skip `verb_only` rows for 형용사 cards.
#[derive(Serialize, ToSchema)]
pub struct InflectionFormValue {
    pub slug: String,
    pub label_en: String,
    pub label_ko: String,
    pub ending_ko: String,
    pub category_slug: String,
    pub category_label_en: String,
    pub category_label_ko: String,
    pub verb_only: bool,
    pub sort_order: i64,
}

async fn fetch_inflection_forms(pool: &SqlitePool) -> Result<Vec<InflectionFormValue>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT f.slug, f.label_en, f.label_ko, f.ending_ko, f.verb_only, f.sort_order,
               c.slug as category_slug, c.label_en as category_label_en, c.label_ko as category_label_ko
        FROM inflection_forms f
        JOIN inflection_categories c ON f.category_id = c.id
        ORDER BY f.sort_order
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|row| InflectionFormValue {
            slug: row.get("slug"),
            label_en: row.get("label_en"),
            label_ko: row.get("label_ko"),
            ending_ko: row.get("ending_ko"),
            category_slug: row.get("category_slug"),
            category_label_en: row.get("category_label_en"),
            category_label_ko: row.get("category_label_ko"),
            verb_only: row.get::<i64, _>("verb_only") != 0,
            sort_order: row.get("sort_order"),
        })
        .collect())
}

#[derive(Deserialize, Default, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FieldValuesQuery {
    /// Comma-separated subset of fields to return (e.g. `fields=pos,tense`).
    /// Omit entirely to get all six.
    #[serde(default, deserialize_with = "deserialize_field_list")]
    fields: Vec<FieldName>,
}

/// Same comma-joined-string convention as `NextCardQuery::exclude` (see
/// `cards::next`), for the same reason: `serde_urlencoded` has no support
/// for repeated-key arrays. Delegates each comma-split piece to
/// `FieldName`'s own derived `Deserialize` (via `IntoDeserializer`) rather
/// than hand-matching slugs a second time, so the accepted `?fields=` values
/// can't drift from `FieldName`'s `rename_all` and the generated schema.
fn deserialize_field_list<'de, D>(deserializer: D) -> Result<Vec<FieldName>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    raw.split(',')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let deserializer: StrDeserializer<serde::de::value::Error> =
                s.trim().into_deserializer();
            FieldName::deserialize(deserializer).map_err(serde::de::Error::custom)
        })
        .collect()
}

async fn fetch_field_values(
    pool: &SqlitePool,
    table: &str,
    has_tooltip: bool,
    has_rank: bool,
    has_endings: bool,
) -> Result<Vec<FieldValue>, AppError> {
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
        .map(|row| FieldValue {
            slug: row.get("slug"),
            label: row.get("label"),
            tooltip: if has_tooltip { row.get("tooltip") } else { None },
            rank: if has_rank { row.get("rank") } else { None },
            endings: if has_endings { row.get("endings") } else { None },
        })
        .collect())
}

#[derive(Serialize, ToSchema)]
pub struct FieldValues {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos: Option<Vec<FieldValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_type: Option<Vec<FieldValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<Vec<FieldValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speech_level: Option<Vec<FieldValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tense: Option<Vec<FieldValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grammar_pattern: Option<Vec<FieldValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inflection_form: Option<Vec<InflectionFormValue>>,
}

#[utoipa::path(
    get,
    path = "/api/cards/field-values",
    params(FieldValuesQuery),
    responses(
        (status = 200, description = "Lookup-table values for dropdowns/enum displays. \
            Fields not requested via `?fields=` are omitted entirely rather than sent empty.", body = FieldValues),
        (status = 400, description = "Unknown name in `?fields=`", body = crate::error::ErrorResponse),
        (status = 401, description = "Missing/invalid JWT", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "cards"
)]
pub async fn list_field_values(
    State(pool): State<SqlitePool>,
    _auth: crate::auth::AuthUser,
    AppQuery(params): AppQuery<FieldValuesQuery>,
) -> Result<Json<FieldValues>, AppError> {
    let wanted = |field: FieldName| params.fields.is_empty() || params.fields.contains(&field);

    let pos = if wanted(FieldName::Pos) {
        Some(fetch_field_values(&pool, "parts_of_speech", false, false, false).await?)
    } else {
        None
    };
    let origin_type = if wanted(FieldName::OriginType) {
        Some(fetch_field_values(&pool, "origin_types", false, false, false).await?)
    } else {
        None
    };
    let grade = if wanted(FieldName::Grade) {
        Some(fetch_field_values(&pool, "grades", false, true, false).await?)
    } else {
        None
    };
    let speech_level = if wanted(FieldName::SpeechLevel) {
        Some(fetch_field_values(&pool, "speech_levels", false, false, false).await?)
    } else {
        None
    };
    let tense = if wanted(FieldName::Tense) {
        Some(fetch_field_values(&pool, "tenses", false, false, false).await?)
    } else {
        None
    };
    let grammar_pattern = if wanted(FieldName::GrammarPattern) {
        Some(fetch_field_values(&pool, "grammar_patterns", true, false, true).await?)
    } else {
        None
    };
    let inflection_form = if wanted(FieldName::InflectionForm) {
        Some(fetch_inflection_forms(&pool).await?)
    } else {
        None
    };

    Ok(Json(FieldValues {
        pos,
        origin_type,
        grade,
        speech_level,
        tense,
        grammar_pattern,
        inflection_form,
    }))
}
