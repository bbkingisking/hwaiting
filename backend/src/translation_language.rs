//! Explicit content language, independent of interface labels.
use crate::{auth::AuthUser, error::AppError};
use axum::{
    Json,
    extract::{FromRequestParts, Query, State},
    http::request::Parts,
};
use sqlx::{Sqlite, SqlitePool};

#[derive(Clone, Debug)]
pub struct TranslationLanguage(pub String);
impl<S: Send + Sync> FromRequestParts<S> for TranslationLanguage {
    type Rejection = AppError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, AppError> {
        let Query(pairs) = Query::<Vec<(String, String)>>::from_request_parts(parts, state)
            .await
            .map_err(|_| AppError::TranslationLanguage("invalid_translation_language"))?;
        let values: Vec<_> = pairs
            .iter()
            .filter(|(key, _)| key == "translation_language")
            .collect();
        if values.is_empty() {
            return Err(AppError::TranslationLanguage(
                "missing_translation_language",
            ));
        }
        if values.len() != 1 {
            return Err(AppError::TranslationLanguage(
                "invalid_translation_language",
            ));
        }
        let code = &values[0].1;
        if code.len() != 3 || !code.bytes().all(|c| c.is_ascii_lowercase()) {
            return Err(AppError::TranslationLanguage(
                "invalid_translation_language",
            ));
        }
        Ok(Self(code.clone()))
    }
}
impl TranslationLanguage {
    pub async fn id<'e, E>(&self, executor: E) -> Result<i64, AppError>
    where
        E: sqlx::Executor<'e, Database = Sqlite>,
    {
        sqlx::query_scalar("SELECT id FROM languages WHERE slug = ?")
            .bind(&self.0)
            .fetch_optional(executor)
            .await?
            .ok_or(AppError::TranslationLanguage(
                "unknown_translation_language",
            ))
    }
}

#[derive(serde::Serialize, utoipa::ToSchema, sqlx::FromRow, Debug)]
pub struct AvailableTranslationLanguage {
    pub code: String,
    /// Distinct cards with at least one complete sentence in this language.
    pub card_count: i64,
}

#[utoipa::path(get, path = "/api/cards/translation-languages", responses((status = 200, body = Vec<AvailableTranslationLanguage>)), security(("cookie_auth" = [])), tag = "cards")]
pub async fn available_translation_languages(
    State(pool): State<SqlitePool>,
    _auth: AuthUser,
) -> Result<Json<Vec<AvailableTranslationLanguage>>, AppError> {
    Ok(Json(
        sqlx::query_as(
            "SELECT l.slug AS code, COUNT(DISTINCT ct.card_id) AS card_count
         FROM languages l
         JOIN cards_translations ct ON ct.language_id = l.id
         JOIN sentences_translations st ON st.card_id = ct.card_id AND st.language_id = l.id
         GROUP BY l.id, l.slug ORDER BY l.slug",
        )
        .fetch_all(&pool)
        .await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn query_contract_is_explicit_and_errors_have_codes() {
        let app = Router::new().route(
            "/",
            get(|translation_language: TranslationLanguage| async move { translation_language.0 }),
        );
        for (query, code) in [
            ("", "missing_translation_language"),
            ("?translation_language=en", "invalid_translation_language"),
            ("?translation_language=ENG", "invalid_translation_language"),
            ("?translation_language=", "invalid_translation_language"),
            (
                "?translation_language=eng&translation_language=fra",
                "invalid_translation_language",
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/{query}"))
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["code"], code);
        }
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/?translation_language=fra")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn discovery_requires_complete_content_and_lookup_rejects_unknown_codes() {
        let pool = crate::test_support::test_pool().await;
        let user = crate::test_support::test_user(&pool).await;
        sqlx::query("INSERT INTO languages (slug) VALUES ('fra')")
            .execute(&pool)
            .await
            .unwrap();
        assert!(matches!(
            TranslationLanguage("zzz".into()).id(&pool).await,
            Err(AppError::TranslationLanguage(
                "unknown_translation_language"
            ))
        ));
        let languages = available_translation_languages(State(pool.clone()), AuthUser(user))
            .await
            .unwrap()
            .0;
        assert_eq!(
            languages
                .iter()
                .map(|language| language.code.as_str())
                .collect::<Vec<_>>(),
            vec!["eng"]
        );
        assert_eq!(languages[0].card_count, 50);
        crate::test_support::translated_sentence(
            &pool,
            1,
            "새 답입니다.",
            "답",
            "Another sentence.",
        )
        .await;
        let counts = available_translation_languages(State(pool.clone()), AuthUser(user))
            .await
            .unwrap()
            .0;
        assert_eq!(
            counts[0].card_count, 50,
            "extra sentences must not inflate the card count"
        );
        let queue = crate::cards::get_review_queue(
            State(pool),
            AuthUser(user),
            crate::error::AppQuery(crate::cards::ReviewQueueQuery::default()),
            TranslationLanguage("fra".into()),
        )
        .await
        .unwrap();
        assert_eq!(
            serde_json::to_value(queue.0).unwrap()["cards"],
            serde_json::json!([])
        );
    }

    #[tokio::test]
    async fn french_queue_token_search_and_edit_preserve_language() {
        use crate::error::{AppJson, AppPath, AppQuery};
        let pool = crate::test_support::test_pool().await;
        let admin = crate::test_support::test_admin_user(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        sqlx::raw_sql(r#"
            INSERT INTO languages (id, slug) VALUES (100, 'fra');
            INSERT INTO sentences (id, card_id, text, required_language_id) VALUES (-1, 1, '새 답입니다.', 100);
            INSERT INTO targets (sentence_id, form) VALUES (-1, '답');
            INSERT INTO cards_translations (card_id, language_id, trans_word, required_sentence_id) VALUES (1, 100, 'réponse', -1);
            INSERT INTO sentences_translations (sentence_id, card_id, language_id, translation) VALUES (-1, 1, 100, 'Une réponse.');
        "#).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let views = crate::admin::get_sentence_translations(
            crate::auth::AdminUser(admin),
            State(pool.clone()),
            AppPath((1, -1)),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].translation_language, "fra");
        let absent = crate::admin::get_sentence_translations(
            crate::auth::AdminUser(admin),
            State(pool.clone()),
            AppPath((2, -1)),
        )
        .await;
        assert!(matches!(absent, Err(AppError::NotFound)));
        let languages = available_translation_languages(State(pool.clone()), AuthUser(admin))
            .await
            .unwrap()
            .0;
        assert_eq!(
            languages
                .iter()
                .map(|language| language.code.as_str())
                .collect::<Vec<_>>(),
            vec!["eng", "fra"]
        );
        assert_eq!(languages[1].card_count, 1);
        let queue = crate::cards::get_review_queue(
            State(pool.clone()),
            AuthUser(admin),
            AppQuery(crate::cards::ReviewQueueQuery::default()),
            TranslationLanguage("fra".into()),
        )
        .await
        .unwrap()
        .0;
        let queue = serde_json::to_value(queue).unwrap();
        assert_eq!(queue["translation_language"], "fra");
        assert_eq!(queue["cards"].as_array().unwrap().len(), 1);
        assert_eq!(queue["cards"][0]["sentence_id"], -1);
        assert_eq!(queue["cards"][0]["trans_word"], "réponse");
        let token = queue["cards"][0]["attempt_token"].as_str().unwrap();
        let mut tx = pool.begin().await.unwrap();
        let claims = crate::cards::validate_attempt(&mut tx, token, admin, Some(1))
            .await
            .unwrap();
        assert_eq!(claims.translation_language, "fra");
        tx.rollback().await.unwrap();
        let result = crate::cards::check_answer(
            State(pool.clone()),
            AppPath(1),
            AuthUser(admin),
            AppJson(crate::cards::CheckRequest {
                answer: "답".into(),
                attempt_token: token.into(),
            }),
        )
        .await
        .unwrap();
        assert!(result.0.correct);
        let found = crate::admin::search_cards(
            crate::auth::AdminUser(admin),
            State(pool.clone()),
            AppQuery(crate::admin::SearchCardsQuery { q: "1".into() }),
            TranslationLanguage("fra".into()),
        )
        .await
        .unwrap();
        assert_eq!(found.0.cards.len(), 1);
        let before: String = sqlx::query_scalar(
            "SELECT trans_word FROM cards_translations WHERE card_id = 1 AND language_id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let patch = crate::admin::UpdateCardRequest {
            sentence_id: -1,
            expected_content_revision: 0,
            trans_word: Some("nouvelle réponse".into()),
            ..Default::default()
        };
        let _ = crate::admin::edit_card(
            crate::auth::AdminUser(admin),
            State(pool.clone()),
            AppPath(1),
            AppJson(patch),
            TranslationLanguage("fra".into()),
        )
        .await
        .unwrap();
        let after: String = sqlx::query_scalar(
            "SELECT trans_word FROM cards_translations WHERE card_id = 1 AND language_id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(before, after);
        let french: String = sqlx::query_scalar(
            "SELECT trans_word FROM cards_translations WHERE card_id = 1 AND language_id = 100",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(french, "nouvelle réponse");
        let absent = crate::admin::UpdateCardRequest {
            sentence_id: -1,
            expected_content_revision: 1,
            ..Default::default()
        };
        let missing = crate::admin::edit_card(
            crate::auth::AdminUser(admin),
            State(pool.clone()),
            AppPath(1),
            AppJson(absent),
            TranslationLanguage("eng".into()),
        )
        .await;
        assert!(matches!(missing, Err(AppError::NotFound)));
    }
}
