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
    /// A grammar pattern's literal conjugation endings (e.g. `["-네", "-네요",
    /// "-군", "-군요"]`), one entry per `grammar_patterns_endings` row -
    /// empty for every other field (which has no such concept at all) and
    /// for a grammar pattern with none recorded yet.
    pub endings: Vec<String>,
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
/// skip a row whose `restricted_to_pos` doesn't match the card's own `pos`.
#[derive(Serialize, ToSchema)]
pub struct InflectionFormValue {
    pub slug: String,
    pub label_en: String,
    pub label_ko: String,
    pub ending_ko: String,
    pub category_slug: String,
    pub category_label_en: String,
    pub category_label_ko: String,
    /// The `parts_of_speech` slug this form is restricted to (e.g. `"verb"`
    /// for `adnominal_present_verb`, `"adjective"` for
    /// `adnominal_present_adj`), or `None` if every part of speech gets this
    /// form - see migration 20240101000048, which replaced a single
    /// `verb_only` boolean with this fk once a form needed the opposite
    /// restriction direction.
    pub restricted_to_pos: Option<String>,
    pub sort_order: i64,
}

async fn fetch_inflection_forms(pool: &SqlitePool) -> Result<Vec<InflectionFormValue>, AppError> {
    // conjugation_matrix_forms_labels is deliberately sparse (see migration
    // 20240101000046) - a row exists only where category + speech level
    // don't already say everything about a form (e.g. `future_haeyo` needs
    // "Future" spelled out because its category is "future" already, but
    // `present_haeyo` needs nothing beyond its category's own "Present").
    // COALESCE falls back to the category's label wherever the sparse
    // per-form one is absent, composing exactly the label the old
    // (now-dropped) `inflection_forms.label_en/label_ko` columns used to
    // store directly - see that migration's doc comment for why baking it
    // back into a stored column would reintroduce the duplication this
    // schema was redesigned to remove.
    let eng_id = crate::enum_lookup::eng_language_id(pool).await?;
    let kor_id = crate::enum_lookup::kor_language_id(pool).await?;

    let rows = sqlx::query(
        r#"
        SELECT f.slug, f.ending as ending_ko, f.sort_order,
               rp.slug as restricted_to_pos,
               c.slug as category_slug,
               cl_en.label as category_label_en, cl_ko.label as category_label_ko,
               COALESCE(fl_en.label, cl_en.label) as label_en,
               COALESCE(fl_ko.label, cl_ko.label) as label_ko
        FROM conjugation_matrix_forms f
        JOIN conjugation_matrix_categories c ON c.id = f.category_id
        INNER JOIN conjugation_matrix_categories_labels cl_en ON cl_en.category_id = c.id AND cl_en.language_id = ?
        INNER JOIN conjugation_matrix_categories_labels cl_ko ON cl_ko.category_id = c.id AND cl_ko.language_id = ?
        LEFT JOIN conjugation_matrix_forms_labels fl_en ON fl_en.form_id = f.id AND fl_en.language_id = ?
        LEFT JOIN conjugation_matrix_forms_labels fl_ko ON fl_ko.form_id = f.id AND fl_ko.language_id = ?
        LEFT JOIN parts_of_speech rp ON rp.id = f.restricted_to_pos_id
        ORDER BY f.sort_order
        "#,
    )
    .bind(eng_id)
    .bind(kor_id)
    .bind(eng_id)
    .bind(kor_id)
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
            restricted_to_pos: row.get("restricted_to_pos"),
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
    // `label`/`tooltip` moved off the lookup table itself and into
    // per-language `_labels`/`_tooltips` side tables (migrations
    // 20240101000041/42) - `labels_table` is the one place the table ->
    // child-table/fk-column mapping lives, shared with
    // `enum_lookup::resolve_or_create_id` so the two can't drift apart.
    // Every value below is still English-only, matching the app's current
    // all-English status quo (see `enum_lookup`'s doc comment).
    let (labels_table, fk_column) = crate::enum_lookup::labels_table(table);
    let eng_id = crate::enum_lookup::eng_language_id(pool).await?;

    let tooltip_expr = if has_tooltip {
        "(SELECT tooltip FROM grammar_patterns_tooltips \
          WHERE grammar_pattern_id = t.id AND language_id = ?) AS tooltip"
    } else {
        "NULL AS tooltip"
    };
    let rank_expr = if has_rank { "t.rank AS rank" } else { "NULL AS rank" };
    // `grammar_patterns_endings` (migration 20240101000040) is one row per
    // literal ending - `json_group_array` folds a pattern's rows into a
    // JSON array in the same `ORDER BY id` they were seeded in, so
    // `FieldValue.endings` ships the normalized list itself rather than a
    // hand-formatted display string. `COALESCE` covers the zero-row case,
    // which `json_group_array` would otherwise leave NULL instead of `[]`.
    let endings_expr = if has_endings {
        "COALESCE((SELECT json_group_array(ending) FROM \
          (SELECT ending FROM grammar_patterns_endings \
           WHERE grammar_pattern_id = t.id ORDER BY id)), '[]') AS endings_json"
    } else {
        "'[]' AS endings_json"
    };

    let sql = format!(
        "SELECT t.slug, l.label, {tooltip_expr}, {rank_expr}, {endings_expr} \
         FROM {table} t \
         INNER JOIN {labels_table} l ON l.{fk_column} = t.id AND l.language_id = ? \
         ORDER BY t.id"
    );

    let mut query = sqlx::query(&sql);
    if has_tooltip {
        query = query.bind(eng_id);
    }
    query = query.bind(eng_id);

    let rows = query.fetch_all(pool).await?;

    Ok(rows
        .iter()
        .map(|row| {
            let endings_json: String = row.get("endings_json");
            FieldValue {
                slug: row.get("slug"),
                label: row.get("label"),
                tooltip: row.get("tooltip"),
                rank: row.get("rank"),
                endings: serde_json::from_str(&endings_json).unwrap_or_default(),
            }
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
