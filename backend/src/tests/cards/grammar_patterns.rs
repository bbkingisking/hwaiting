use super::*;
use crate::{
    auth::AuthUser,
    cards,
    error::{AppJson, AppPath, AppQuery},
    test_support::{test_pool, test_review_user},
};

#[tokio::test]
async fn selected_grammar_content_is_embedded_without_revealing_endings() {
    let pool = test_pool().await;
    let user = test_review_user(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql("INSERT INTO grammar_patterns (id,slug) VALUES (1000,'new-pattern'); INSERT INTO grammar_patterns_labels (grammar_pattern_id,language_id,label) VALUES (1000,1,'New authored pattern'); INSERT INTO grammar_patterns_tooltips (grammar_pattern_id,language_id,tooltip) VALUES (1000,1,'An authored explanation.'); INSERT INTO grammar_patterns_endings (grammar_pattern_id,ending) VALUES (1000,'-example'); UPDATE targets SET grammar_pattern_id=1000 WHERE sentence_id=1;").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let queue = cards::get_review_queue(
        State(pool.clone()),
        AuthUser(user),
        AppQuery(cards::ReviewQueueQuery { limit: None }),
    )
    .await
    .unwrap()
    .0;
    let queue = serde_json::to_value(queue).unwrap();
    let entry = queue["cards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["card"]["card_id"] == 1)
        .unwrap();
    assert_eq!(
        entry["card"]["grammar_pattern"],
        serde_json::json!({"slug":"new-pattern","name":"New authored pattern","explanation":"An authored explanation."})
    );
    assert!(!entry.to_string().contains("-example"));
    let detail = crate::admin::get_admin_card(AdminUser(user), State(pool.clone()), AppPath(1))
        .await
        .unwrap()
        .0;
    assert_eq!(
        detail.sentence.target.grammar_pattern.unwrap().slug,
        "new-pattern"
    );
    let catalog = serde_json::to_value(
        list_grammar_patterns(AdminUser(user), State(pool.clone()))
            .await
            .unwrap()
            .0,
    )
    .unwrap();
    assert_eq!(
        catalog
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["slug"] == "new-pattern")
            .unwrap()["endings"],
        serde_json::json!(["-example"])
    );
    let target: String = sqlx::query_scalar("SELECT form FROM targets WHERE sentence_id=1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let answer = cards::check_answer(
        State(pool),
        AuthUser(user),
        AppJson(cards::CheckRequest {
            answer: target,
            attempt_token: entry["review"]["attempt_token"].as_str().unwrap().into(),
        }),
    )
    .await
    .unwrap()
    .1
    .0;
    assert!(answer.correct);
    assert_eq!(answer.reveal.grammar_pattern_endings, vec!["-example"]);
}

#[tokio::test]
async fn grammar_catalog_requires_admin_authorization() {
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt;
    let pool = test_pool().await;
    let user = test_review_user(&pool).await;
    let token = crate::auth::generate_token(user).unwrap();
    let app = Router::new()
        .route("/api/admin/grammar-patterns", get(list_grammar_patterns))
        .with_state(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/admin/grammar-patterns")
                .header("cookie", format!("hwaiting-auth={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
