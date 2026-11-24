//! Saved review content language and explicit admin translation selection.
use crate::{auth::AuthUser, error::AppError};
use axum::{Json, extract::State};
use sqlx::{Sqlite, SqlitePool};

#[derive(Clone, Debug)]
pub struct TranslationLanguage(pub String);
impl TranslationLanguage {
    /// Resolve the account's preferred content language without an implicit fallback.
    pub async fn for_user<'e, E>(executor: E, user_id: i64) -> Result<Self, AppError>
    where
        E: sqlx::Executor<'e, Database = Sqlite>,
    {
        let code: Option<String> =
            sqlx::query_scalar("SELECT translation_language FROM users_settings WHERE user_id = ?")
                .bind(user_id)
                .fetch_optional(executor)
                .await?
                .flatten();
        code.map(Self)
            .ok_or(AppError::TranslationLanguageNotConfigured)
    }

    pub async fn id<'e, E>(&self, executor: E) -> Result<i64, AppError>
    where
        E: sqlx::Executor<'e, Database = Sqlite>,
    {
        sqlx::query_scalar("SELECT id FROM languages WHERE slug = ?")
            .bind(&self.0)
            .fetch_optional(executor)
            .await?
            .ok_or(AppError::UnknownTranslationLanguage)
    }
}

#[derive(serde::Serialize, utoipa::ToSchema, sqlx::FromRow, Debug)]
pub struct AvailableTranslationLanguage {
    pub code: String,
    /// Distinct cards with at least one complete sentence in this language.
    pub card_count: i64,
}

#[derive(serde::Serialize, utoipa::ToSchema, Debug)]
pub struct AvailableTranslationLanguagesResponse {
    pub languages: Vec<AvailableTranslationLanguage>,
}

#[utoipa::path(get, path = "/api/cards/languages", description = "Available translation languages for Korean card content.", responses((status = 200, description = "Languages with at least one complete card, and how many cards each has", body = AvailableTranslationLanguagesResponse)), security(("cookie_auth" = [])), tag = "cards")]
pub async fn available_translation_languages(
    State(pool): State<SqlitePool>,
    _auth: AuthUser,
) -> Result<Json<AvailableTranslationLanguagesResponse>, AppError> {
    Ok(Json(AvailableTranslationLanguagesResponse {
        languages: sqlx::query_as(
            "SELECT l.slug AS code, COUNT(DISTINCT ct.card_id) AS card_count
         FROM languages l
         JOIN cards_translations ct ON ct.language_id = l.id
         JOIN sentences_translations st ON st.card_id = ct.card_id AND st.language_id = l.id
         GROUP BY l.id, l.slug ORDER BY l.slug",
        )
        .fetch_all(&pool)
        .await?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
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
            Err(AppError::UnknownTranslationLanguage)
        ));
        let languages = available_translation_languages(State(pool.clone()), AuthUser(user))
            .await
            .unwrap()
            .0
            .languages;
        assert_eq!(
            languages
                .iter()
                .map(|language| language.code.as_str())
                .collect::<Vec<_>>(),
            vec!["eng"]
        );
        assert_eq!(languages[0].card_count, 50);
        crate::user::ensure_settings_row(&pool, user).await.unwrap();
        sqlx::query("UPDATE users_settings SET translation_language = 'fra' WHERE user_id = ?")
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        let queue = crate::cards::get_review_queue(
            State(pool),
            AuthUser(user),
            crate::error::AppQuery(crate::cards::ReviewQueueQuery::default()),
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
            UPDATE sentences SET text = '새 답입니다.' WHERE id = 1;
            UPDATE targets SET form = '답' WHERE sentence_id = 1;
            INSERT INTO cards_translations (card_id, language_id, trans_word, required_sentence_id) VALUES (1, 100, 'réponse', 1);
            INSERT INTO sentences_translations (sentence_id, card_id, language_id, translation) VALUES (1, 1, 100, 'Une réponse.');
        "#).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let detail = crate::admin::get_admin_card(
            crate::auth::AdminUser(admin),
            State(pool.clone()),
            AppPath(1),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(detail.sentence.translations["fra"], "Une réponse.");
        assert_eq!(detail.translations["fra"].word, "réponse");
        let languages = available_translation_languages(State(pool.clone()), AuthUser(admin))
            .await
            .unwrap()
            .0
            .languages;
        assert_eq!(
            languages
                .iter()
                .map(|language| language.code.as_str())
                .collect::<Vec<_>>(),
            vec!["eng", "fra"]
        );
        assert_eq!(languages[1].card_count, 1);
        crate::user::ensure_settings_row(&pool, admin)
            .await
            .unwrap();
        sqlx::query("UPDATE users_settings SET translation_language = 'fra' WHERE user_id = ?")
            .bind(admin)
            .execute(&pool)
            .await
            .unwrap();
        let queue = crate::cards::get_review_queue(
            State(pool.clone()),
            AuthUser(admin),
            AppQuery(crate::cards::ReviewQueueQuery::default()),
        )
        .await
        .unwrap()
        .0;
        let queue = serde_json::to_value(queue).unwrap();
        assert!(queue.get("translation_language").is_none());
        assert_eq!(queue["cards"].as_array().unwrap().len(), 1);
        assert_eq!(queue["cards"][0]["card"]["sentence_id"], 1);
        assert_eq!(queue["cards"][0]["card"]["trans_word"], "réponse");
        let token = queue["cards"][0]["review"]["attempt_token"]
            .as_str()
            .unwrap();
        // A preference change must not reinterpret an already-issued attempt.
        sqlx::query("UPDATE users_settings SET translation_language = 'eng' WHERE user_id = ?")
            .bind(admin)
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let claims = crate::cards::validate_attempt(&mut tx, token, admin, Some(1))
            .await
            .unwrap();
        assert_eq!(claims.translation_language, "fra");
        tx.rollback().await.unwrap();
        let result = crate::cards::check_answer(
            State(pool.clone()),
            AuthUser(admin),
            AppJson(crate::cards::CheckRequest {
                answer: "답".into(),
                attempt_token: token.into(),
            }),
        )
        .await
        .unwrap();
        assert!(result.1.0.correct);
        let found = crate::admin::search_card_summaries(
            crate::auth::AdminUser(admin),
            State(pool.clone()),
            AppQuery(crate::admin::SearchCardsQuery { q: "1".into() }),
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
            sentence_id: 1,
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
            sentence_id: 2,
            expected_content_revision: 0,
            ..Default::default()
        };
        let missing = crate::admin::edit_card(
            crate::auth::AdminUser(admin),
            State(pool.clone()),
            AppPath(2),
            AppJson(absent),
            TranslationLanguage("fra".into()),
        )
        .await;
        assert!(matches!(missing, Err(AppError::NotFound)));
    }
    #[tokio::test]
    async fn user_content_requires_a_saved_language() {
        let pool = crate::test_support::test_pool().await;
        let user = crate::test_support::test_user(&pool).await;
        let result = crate::cards::get_review_queue(
            State(pool.clone()),
            AuthUser(user),
            crate::error::AppQuery(crate::cards::ReviewQueueQuery::default()),
        )
        .await;
        assert!(matches!(
            result,
            Err(AppError::TranslationLanguageNotConfigured)
        ));
        crate::user::ensure_settings_row(&pool, user).await.unwrap();
        assert!(matches!(
            TranslationLanguage::for_user(&pool, user).await,
            Err(AppError::TranslationLanguageNotConfigured)
        ));
    }
}
