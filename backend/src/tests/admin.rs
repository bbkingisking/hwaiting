use super::*;

// Only the fields relevant to the three-state distinction are set in
// these fixtures; the rest of `UpdateCardRequest` deserializes to `None`
// (omitted) via each field's own `Option` default, which is not what's
// under test here.

#[test]
fn key_absent_is_none() {
    let req: UpdateCardRequest =
        serde_json::from_str(r#"{"expected_content_revision":0}"#).unwrap();
    assert!(req.hanja.is_none());
}

#[test]
fn key_explicit_null_is_some_none() {
    // This is the case the hanja-edit-not-persisting bug got wrong:
    // an explicit `null` (clear the column) used to be indistinguishable
    // from the key being absent (leave the column alone).
    let req: UpdateCardRequest =
        serde_json::from_str(r#"{"expected_content_revision":0,"hanja": null}"#).unwrap();
    assert_eq!(req.hanja, Some(None));
}

#[test]
fn key_with_value_is_some_some() {
    let req: UpdateCardRequest =
        serde_json::from_str(r#"{"expected_content_revision":0,"hanja": "漢字"}"#).unwrap();
    assert_eq!(req.hanja, Some(Some("漢字".to_string())));
}

// No separate bool-field or plain-Option-field variants: `double_option`
// is generic over `T`, and a plain `Option<T>` field is just derived
// serde with no custom code of ours behind it - neither would exercise
// anything the three cases above don't already cover.

// --- edit_card (handler-level, in-process SQLite,crate::translation_language::TranslationLanguage("eng".into())) -------------------------

use crate::test_support::{test_admin_user, test_pool};

async fn sentence_and_target(pool: &SqlitePool, card_id: i64) -> (i64, String, String) {
    sqlx::query_as(
        "SELECT s.id, s.text, tg.form FROM sentences s JOIN targets tg ON tg.sentence_id = s.id WHERE s.card_id = ?",
    )
    .bind(card_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn target_not_in_existing_sentence_is_rejected() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;
    let card_id = 1;

    let result = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(card_id),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            target: Some("절대로존재하지않는단어".to_string()),
            ..Default::default()
        }),
        crate::translation_language::TranslationLanguage("eng".into()),
    )
    .await;

    assert!(matches!(result, Err(AppError::BadRequest(_))));
}

#[tokio::test]
async fn new_sentence_missing_the_existing_target_is_rejected() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;
    let card_id = 1;

    let result = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(card_id),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            sentence: Some("이 문장에는 정답이 전혀 없습니다".to_string()),
            ..Default::default()
        }),
        crate::translation_language::TranslationLanguage("eng".into()),
    )
    .await;

    assert!(matches!(result, Err(AppError::BadRequest(_))));
}

#[tokio::test]
async fn sentence_update_is_validated_against_the_unchanged_target() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;
    let card_id = 1;
    let (sentence_id, _old_sentence, target) = sentence_and_target(&pool, card_id).await;
    let new_sentence = format!("{} 그리고 다른 문장입니다", target);

    let result = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(card_id),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            sentence: Some(new_sentence.clone()),
            ..Default::default()
        }),
        crate::translation_language::TranslationLanguage("eng".into()),
    )
    .await;
    let result = result.unwrap().0;
    assert_eq!(result.card.front.sentence_id, sentence_id);
    assert_eq!(result.card.back.sentence, new_sentence);
    assert_eq!(result.card.back.target, target);
    assert_eq!(result.card.front.sentence_before, "");
    assert_eq!(result.card.front.sentence_after, " 그리고 다른 문장입니다");

    let stored: String = sqlx::query_scalar("SELECT text FROM sentences WHERE id = ?")
        .bind(sentence_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, new_sentence);
}

#[tokio::test]
async fn edit_returns_the_same_canonical_projection_as_search() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;
    let response = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(1),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            sentence: Some("새 답입니다.".into()),
            target: Some("답".into()),
            is_honorific: Some(Some(true)),
            is_humble: Some(Some(true)),
            ..Default::default()
        }),
        crate::translation_language::TranslationLanguage("eng".into()),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(response.card.front.sentence_before, "새 ");
    assert_eq!(response.card.front.sentence_after, "입니다.");
    assert!(response.card.front.inflection_hint.is_honorific);
    assert!(response.card.front.inflection_hint.is_humble);
    let detail = get_admin_card(AdminUser(admin_id), State(pool), AppPath(1))
        .await
        .unwrap()
        .0;
    assert_eq!(detail.word, response.card.back.word);
    assert_eq!(detail.sentence.text, response.card.back.sentence);
    assert_eq!(detail.sentence.target.form, response.card.back.target);
}

#[tokio::test]
async fn alternatives_are_trimmed_and_blanks_dropped() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;
    let card_id = 1;
    let (sentence_id, _, _) = sentence_and_target(&pool, card_id).await;

    let result = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(card_id),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            alternatives: Some(vec![
                "  alt1  ".to_string(),
                "".to_string(),
                "   ".to_string(),
                "alt2".to_string(),
            ]),
            ..Default::default()
        }),
        crate::translation_language::TranslationLanguage("eng".into()),
    )
    .await;
    assert!(result.is_ok());

    let mut stored: Vec<String> = sqlx::query_scalar(
        "SELECT alt_target FROM targets_alternatives WHERE sentence_id = ? ORDER BY alt_target",
    )
    .bind(sentence_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    stored.sort();
    assert_eq!(stored, vec!["alt1".to_string(), "alt2".to_string()]);
}

#[tokio::test]
async fn editing_a_nonexistent_card_is_not_found() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;

    let result = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(999999),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            word: Some("x".to_string()),
            ..Default::default()
        }),
        crate::translation_language::TranslationLanguage("eng".into()),
    )
    .await;

    assert!(matches!(result, Err(AppError::NotFound)));
}

#[test]
fn edit_requires_content_revision() {
    assert!(serde_json::from_str::<SaveAdminCardRequest>(r#"{"changes":[]}"#).is_err());
}

#[tokio::test]
async fn stale_edit_cannot_overwrite_content_or_change_other_fields() {
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;
    let first = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(1),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            hanja: Some(Some("first editor".into())),
            ..Default::default()
        }),
        crate::translation_language::TranslationLanguage("eng".into()),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(first.content_revision, 1);
    assert_eq!(first.card.front.content_revision, 1);
    let stale = edit_card(
        AdminUser(admin_id),
        State(pool.clone()),
        AppPath(1),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            trans_word: Some("second editor".into()),
            grammar_pattern: Some(Some("must not create an enum".into())),
            ..Default::default()
        }),
        crate::translation_language::TranslationLanguage("eng".into()),
    )
    .await;
    assert!(matches!(stale, Err(AppError::StaleContent)));
    let current = get_admin_card(AdminUser(admin_id), State(pool.clone()), AppPath(1))
        .await
        .unwrap()
        .0;
    assert_eq!(current.content_revision, first.content_revision);
    assert_eq!(current.hanja, first.card.front.hanja);
    assert_eq!(
        current.translations["eng"].word,
        first.card.front.trans_word
    );
    let retry = edit_card(
        AdminUser(admin_id),
        State(pool),
        AppPath(1),
        AppJson(UpdateCardRequest {
            sentence_id: 1,
            expected_content_revision: 1,
            trans_word: Some("second editor".into()),
            ..Default::default()
        }),
        crate::translation_language::TranslationLanguage("eng".into()),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(retry.content_revision, 2);
    assert_eq!(retry.card.front.hanja.as_deref(), Some("first editor"));
    assert_eq!(retry.card.front.trans_word, "second editor");
}

#[tokio::test]
async fn complete_editor_reads_all_translations_and_saves_atomically() {
    let pool = test_pool().await;
    let admin = crate::test_support::test_admin_user(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql("INSERT INTO languages (id, slug) VALUES (100, 'fra'); INSERT INTO cards_translations (card_id, language_id, trans_word, required_sentence_id) VALUES (1, 100, 'chose', 1); INSERT INTO sentences_translations (sentence_id, card_id, language_id, translation) VALUES (1, 1, 100, 'Une chose.');").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let detail = get_admin_card(AdminUser(admin), State(pool.clone()), AppPath(1))
        .await
        .unwrap()
        .0;
    assert_eq!(detail.sentence.sentence_id, 1);
    assert!(detail.translations.contains_key("eng"));
    assert_eq!(detail.translations["fra"].word, "chose");
    assert_eq!(detail.sentence.translations["fra"], "Une chose.");
    let snapshot = serde_json::to_value(&detail).unwrap();
    assert!(snapshot.get("primary_sentence_id").is_none());
    let body = serde_json::json!({"expected_content_revision": 0, "changes": [
        {"translation_language": "eng", "trans_word": "updated word"},
        {"translation_language": "fra", "sentence_translation": "Une chose mise à jour."}
    ]});
    let saved = save_admin_card(
        AdminUser(admin),
        State(pool.clone()),
        AppPath(1),
        AppJson(serde_json::from_value(body).unwrap()),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(saved.card.content_revision, 1);
    assert_eq!(saved.card.translations["eng"].word, "updated word");
    assert_eq!(
        saved.card.sentence.translations["fra"],
        "Une chose mise à jour."
    );
    let invalid = serde_json::json!({"expected_content_revision": 1, "changes": [
        {"translation_language": "eng", "trans_word": "must roll back"},
        {"translation_language": "fra", "sentence_translation": "   "}
    ]});
    // The blank French translation fails only after the English change was
    // written, so this checks that the whole save rolls back.
    let result = save_admin_card(
        AdminUser(admin),
        State(pool.clone()),
        AppPath(1),
        AppJson(serde_json::from_value(invalid).unwrap()),
    )
    .await;
    assert!(
        matches!(&result, Err(AppError::BadRequest(m)) if !m.contains("Duplicate")),
        "{:?}",
        result.as_ref().err()
    );
    let after = get_admin_card(AdminUser(admin), State(pool.clone()), AppPath(1))
        .await
        .unwrap()
        .0;
    assert_eq!(after.content_revision, 1);
    assert_eq!(after.translations["eng"].word, "updated word");
    let stale = serde_json::json!({"expected_content_revision": 0, "changes": []});
    assert!(matches!(
        save_admin_card(
            AdminUser(admin),
            State(pool.clone()),
            AppPath(1),
            AppJson(serde_json::from_value(stale).unwrap())
        )
        .await,
        Err(AppError::StaleContent)
    ));
    let results = search_card_summaries(
        AdminUser(admin),
        State(pool),
        AppQuery(SearchCardsQuery { q: "1".into() }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(results.cards.iter().filter(|c| c.card_id == 1).count(), 1);
}

#[tokio::test]
async fn conjugations_are_self_contained_and_backend_filters_applicability() {
    let pool = test_pool().await;
    let admin = test_admin_user(&pool).await;
    sqlx::raw_sql("UPDATE cards SET pos_id = 1 WHERE id = 1; INSERT INTO conjugation_matrix_cards (card_id, form_id, form) VALUES (1,1,'먹어요'),(1,26,'먹는'),(1,27,'먹은'); DROP TABLE conjugation_matrix_forms_labels; DROP TABLE conjugation_matrix_categories_labels; DROP TABLE parts_of_speech_labels;").execute(&pool).await.unwrap();
    let response = get_card_inflections(AdminUser(admin), State(pool.clone()), AppPath(1))
        .await
        .unwrap()
        .0;
    let json = serde_json::to_value(response).unwrap();
    let forms = json["inflections"].as_array().unwrap();
    assert_eq!(forms.len(), 2);
    let present = forms
        .iter()
        .find(|f| f["form_slug"] == "present_haeyo")
        .unwrap();
    assert_eq!(present["form"], "먹어요");
    assert_eq!(present["ending"], "-아/어/여요");
    assert_eq!(present["tense"], "present");
    assert_eq!(present["speech_level"], "haeyo-che");
    assert!(present.get("category").is_none());
    assert!(present.get("label_en").is_none());
    assert!(
        forms
            .iter()
            .any(|f| f["form_slug"] == "adnominal_present_verb")
    );
    sqlx::query("UPDATE cards SET pos_id = 3 WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();
    let response = serde_json::to_value(
        get_card_inflections(AdminUser(admin), State(pool.clone()), AppPath(1))
            .await
            .unwrap()
            .0,
    )
    .unwrap();
    assert!(
        response["inflections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["form_slug"] == "adnominal_present_adj")
    );
    assert!(
        !response["inflections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["form_slug"] == "adnominal_present_verb")
    );
}

#[tokio::test]
async fn aggregate_save_rejects_conflicting_shared_fields_and_keeps_noops_unchanged() {
    let pool = test_pool().await;
    let admin = test_admin_user(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql("INSERT INTO languages (id,slug) VALUES (100,'fra'); INSERT INTO cards_translations (card_id,language_id,trans_word,required_sentence_id) VALUES (1,100,'chose',1); INSERT INTO sentences_translations (sentence_id,card_id,language_id,translation) VALUES (1,1,100,'Une chose.');").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let conflict = serde_json::json!({"expected_content_revision":0,"changes":[
        {"translation_language":"eng","hanja":"first","trans_word":"must not save"},
        {"translation_language":"fra","hanja":"second"}
    ]});
    assert!(matches!(
        save_admin_card(
            AdminUser(admin),
            State(pool.clone()),
            AppPath(1),
            AppJson(serde_json::from_value(conflict).unwrap())
        )
        .await,
        Err(AppError::BadRequest(_))
    ));
    let card = get_admin_card(AdminUser(admin), State(pool.clone()), AppPath(1))
        .await
        .unwrap()
        .0;
    assert_eq!(card.content_revision, 0);
    assert_ne!(card.translations["eng"].word, "must not save");
    let noop = serde_json::json!({"expected_content_revision":0,"changes":[
        {"translation_language":"eng","trans_word":card.translations["eng"].word},
        {"translation_language":"fra","trans_word":"chose"}
    ]});
    let response = save_admin_card(
        AdminUser(admin),
        State(pool),
        AppPath(1),
        AppJson(serde_json::from_value(noop).unwrap()),
    )
    .await
    .unwrap()
    .0;
    assert!(!response.changed);
    assert_eq!(response.card.content_revision, 0);
}

#[tokio::test]
async fn aggregate_shared_sentence_and_target_do_not_depend_on_language_grouping() {
    let cases = [
        serde_json::json!([
            {"translation_language":"eng","sentence":"새 답입니다.","target":"답","trans_word":"answer"},
            {"translation_language":"fra","sentence_translation":"Une réponse."}
        ]),
        serde_json::json!([
            {"translation_language":"eng","sentence":"새 답입니다.","trans_word":"answer"},
            {"translation_language":"fra","target":"답","sentence_translation":"Une réponse."}
        ]),
        serde_json::json!([
            {"translation_language":"fra","target":"답","sentence_translation":"Une réponse."},
            {"translation_language":"eng","sentence":"새 답입니다.","trans_word":"answer"}
        ]),
    ];
    let mut reference = None;
    for changes in cases {
        let pool = test_pool().await;
        let admin = test_admin_user(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        sqlx::raw_sql("INSERT INTO languages (id,slug) VALUES (100,'fra'); INSERT INTO cards_translations (card_id,language_id,trans_word,required_sentence_id) VALUES (1,100,'chose',1); INSERT INTO sentences_translations (sentence_id,card_id,language_id,translation) VALUES (1,1,100,'Une chose.');").execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let request = serde_json::from_value(serde_json::json!({
            "expected_content_revision":0,"changes":changes
        }))
        .unwrap();
        let saved = save_admin_card(AdminUser(admin), State(pool), AppPath(1), AppJson(request))
            .await
            .unwrap()
            .0;
        assert!(saved.changed);
        assert_eq!(saved.card.content_revision, 1);
        assert_eq!(saved.card.sentence.text, "새 답입니다.");
        assert_eq!(saved.card.sentence.target.form, "답");
        assert_eq!(saved.card.translations["eng"].word, "answer");
        assert_eq!(saved.card.sentence.translations["fra"], "Une réponse.");
        let json = serde_json::to_value(saved.card).unwrap();
        if let Some(reference) = &reference {
            assert_eq!(&json, reference);
        } else {
            reference = Some(json);
        }
    }
}

#[tokio::test]
async fn invalid_merged_sentence_and_target_roll_back_the_entire_save() {
    let pool = test_pool().await;
    let admin = test_admin_user(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql("INSERT INTO languages (id,slug) VALUES (100,'fra'); INSERT INTO cards_translations (card_id,language_id,trans_word,required_sentence_id) VALUES (1,100,'chose',1); INSERT INTO sentences_translations (sentence_id,card_id,language_id,translation) VALUES (1,1,100,'Une chose.');").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let before = serde_json::to_value(
        get_admin_card(AdminUser(admin), State(pool.clone()), AppPath(1))
            .await
            .unwrap()
            .0,
    )
    .unwrap();
    let request = serde_json::from_value(serde_json::json!({"expected_content_revision":0,"changes":[
        {"translation_language":"eng","word":"must roll back","sentence":"새 답입니다.","trans_word":"must also roll back"},
        {"translation_language":"fra","target":"없는표현"}
    ]})).unwrap();
    assert!(matches!(
        save_admin_card(
            AdminUser(admin),
            State(pool.clone()),
            AppPath(1),
            AppJson(request)
        )
        .await,
        Err(AppError::BadRequest(_))
    ));
    let after = serde_json::to_value(
        get_admin_card(AdminUser(admin), State(pool), AppPath(1))
            .await
            .unwrap()
            .0,
    )
    .unwrap();
    assert_eq!(after, before);
}

#[tokio::test]
async fn malformed_and_unknown_language_codes_are_distinct_errors() {
    let pool = test_pool().await;
    let admin = test_admin_user(&pool).await;
    for (code, malformed) in [("EN", true), ("english", true), ("zzz", false)] {
        let request = serde_json::from_value(serde_json::json!({
            "expected_content_revision": 0,
            "changes": [{"translation_language": code, "trans_word": "x"}]
        }))
        .unwrap();
        let result = save_admin_card(
            AdminUser(admin),
            State(pool.clone()),
            AppPath(1),
            AppJson(request),
        )
        .await;
        if malformed {
            assert!(
                matches!(result, Err(AppError::InvalidTranslationLanguage)),
                "{code}"
            );
        } else {
            assert!(
                matches!(result, Err(AppError::UnknownTranslationLanguage)),
                "{code}"
            );
        }
    }
}

#[tokio::test]
async fn blank_text_fields_are_rejected_with_the_field_name() {
    let pool = test_pool().await;
    let admin = test_admin_user(&pool).await;
    for field in [
        "word",
        "trans_word",
        "trans_dfn",
        "sentence",
        "sentence_translation",
        "target",
    ] {
        let request = serde_json::from_value(serde_json::json!({
            "expected_content_revision": 0,
            "changes": [{"translation_language": "eng", field: " \t "}]
        }))
        .unwrap();
        let result = save_admin_card(
            AdminUser(admin),
            State(pool.clone()),
            AppPath(1),
            AppJson(request),
        )
        .await;
        assert!(
            matches!(&result, Err(AppError::BadRequest(m)) if m == &format!("{field} must not be blank")),
            "{field}: {:?}",
            result.as_ref().err()
        );
    }
}
