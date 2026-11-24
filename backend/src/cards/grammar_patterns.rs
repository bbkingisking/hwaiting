//! Authored grammar content, separate from fixed client presentation vocabulary.
use crate::{auth::AuthUser, error::AppError};
use axum::{Json, extract::State};
use serde::Serialize;
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};
use utoipa::ToSchema;

/// The selected pattern's authored content. Literal endings belong to the
/// answer reveal or the resource catalog, never embedded in the pre-answer prompt.
#[derive(Serialize, ToSchema)]
pub struct GrammarPattern {
    pub slug: String,
    pub name: String,
    pub explanation: Option<String>,
}

impl GrammarPattern {
    pub(crate) fn from_row(row: &SqliteRow) -> Option<Self> {
        row.get::<Option<String>, _>("grammar_pattern")
            .map(|slug| Self {
                name: row
                    .get::<Option<String>, _>("grammar_pattern_name")
                    .unwrap_or_else(|| slug.clone()),
                slug,
                explanation: row.get("grammar_pattern_explanation"),
            })
    }
}

#[derive(Serialize, ToSchema)]
pub struct GrammarPatternOption {
    #[serde(flatten)]
    pub pattern: GrammarPattern,
    pub endings: Vec<String>,
}

#[utoipa::path(get, path = "/api/resources/grammar-patterns",
    description = "List authored grammar patterns with English labels, explanations, and accepted endings for authenticated users and card editors.", responses((status = 200, body = Vec<GrammarPatternOption>), (status = 401, body = crate::error::ErrorResponse)), security(("cookie_auth" = [])), tag = "resources")]
pub async fn list_grammar_patterns(
    _auth: AuthUser,
    State(pool): State<SqlitePool>,
) -> Result<Json<Vec<GrammarPatternOption>>, AppError> {
    let rows = sqlx::query(r#"
        SELECT gp.slug AS grammar_pattern,
               gpl.label AS grammar_pattern_name,
               gpt.tooltip AS grammar_pattern_explanation,
               (SELECT json_group_array(ending) FROM
                 (SELECT ending FROM grammar_patterns_endings WHERE grammar_pattern_id = gp.id ORDER BY id)) AS endings
        FROM grammar_patterns gp
        LEFT JOIN grammar_patterns_labels gpl ON gpl.grammar_pattern_id = gp.id
          AND gpl.language_id = (SELECT id FROM languages WHERE slug = 'eng')
        LEFT JOIN grammar_patterns_tooltips gpt ON gpt.grammar_pattern_id = gp.id
          AND gpt.language_id = (SELECT id FROM languages WHERE slug = 'eng')
        ORDER BY gp.id
    "#).fetch_all(&pool).await?;
    Ok(Json(
        rows.iter()
            .map(|row| {
                let endings: String = row.get("endings");
                Ok(GrammarPatternOption {
                    pattern: GrammarPattern::from_row(row).expect("catalog slug is not null"),
                    endings: serde_json::from_str(&endings)
                        .map_err(|e| AppError::Internal(e.to_string()))?,
                })
            })
            .collect::<Result<_, AppError>>()?,
    ))
}

#[cfg(test)]
#[path = "../tests/cards/grammar_patterns.rs"]
mod tests;
