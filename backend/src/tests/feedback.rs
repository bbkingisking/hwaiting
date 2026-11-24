use super::*;
use crate::{
    auth::AuthUser,
    test_support::{test_pool, test_user},
};

#[tokio::test]
async fn submission_records_sentence_user_and_body() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let sentence_id: i64 = sqlx::query_scalar("SELECT id FROM sentences LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let response = submit_feedback(
        State(pool.clone()),
        AuthUser(user_id),
        AppJson(FeedbackRequest {
            sentence_id,
            body: "Please check the tense".into(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(response.0, axum::http::StatusCode::CREATED);
    let stored: (i64, i64, String) =
        sqlx::query_as("SELECT sentence_id, user_id, body FROM feedback WHERE id = ?")
            .bind(response.1.0.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        stored,
        (sentence_id, user_id, "Please check the tense".into())
    );
}

#[tokio::test]
async fn missing_sentence_returns_404_without_feedback() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let result = submit_feedback(
        State(pool.clone()),
        AuthUser(user_id),
        AppJson(FeedbackRequest {
            sentence_id: 999999,
            body: "Check this".into(),
        }),
    )
    .await;
    assert!(matches!(result, Err(AppError::NotFound)));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM feedback")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn feedback_contract_shares_one_path_with_admin_only_listing() {
    use utoipa::OpenApi;
    let spec = serde_json::to_value(crate::openapi::ApiDoc::openapi()).unwrap();
    assert!(spec["paths"].get("/api/cards/{card_id}/comment").is_none());
    assert!(spec["paths"].get("/api/feedback/submit").is_none());
    assert!(spec["paths"].get("/api/admin/feedback").is_none());
    let listing = &spec["paths"]["/api/feedback"]["get"];
    assert_eq!(listing["tags"], serde_json::json!(["feedback"]));
    let deletion = &spec["paths"]["/api/feedback/{feedback_id}"]["delete"];
    assert_eq!(deletion["tags"], serde_json::json!(["feedback"]));
    assert!(deletion["responses"].get("204").is_some());
    assert!(
        listing["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "limit"
                && p["in"] == "query"
                && p["required"] == false
                && p["schema"]["minimum"] == 1)
    );
    assert!(listing["responses"].get("403").is_some());
    let operation = &spec["paths"]["/api/feedback"]["post"];
    assert_eq!(operation["tags"], serde_json::json!(["feedback"]));
    assert!(
        spec["components"]["schemas"]["FeedbackRequest"]["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("sentence_id"))
    );
}

#[tokio::test]
async fn admin_list_returns_all_users_feedback_oldest_first_with_optional_limit() {
    use crate::{auth::AdminUser, test_support::test_admin_user};
    let pool = test_pool().await;
    let admin_id = test_admin_user(&pool).await;
    let user_id = test_user(&pool).await;
    let sentence_id: i64 = sqlx::query_scalar("SELECT id FROM sentences LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    for (id, author, date) in [
        (1, user_id, "2026-11-24 12:00:00"),
        (2, admin_id, "2026-11-23 12:00:00"),
        (3, user_id, "2026-11-23 12:00:00"),
    ] {
        sqlx::query("INSERT INTO feedback (id, sentence_id, user_id, body, created_at) VALUES (?, ?, ?, 'Check tense', ?)")
            .bind(id).bind(sentence_id).bind(author).bind(date).execute(&pool).await.unwrap();
    }
    let all = list_feedback(
        State(pool.clone()),
        AdminUser(admin_id),
        AppQuery(ListFeedbackQuery::default()),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(
        all.feedback.iter().map(|f| f.id).collect::<Vec<_>>(),
        [2, 3, 1]
    );
    assert_eq!(all.feedback[0].user_id, admin_id);
    assert_eq!(all.feedback[0].sentence_id, sentence_id);
    let card_id: i64 = sqlx::query_scalar("SELECT card_id FROM sentences WHERE id = ?")
        .bind(sentence_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(all.feedback[0].card_id, card_id);
    assert_eq!(all.feedback[0].body, "Check tense");
    assert_eq!(all.feedback[0].created_at, "2026-11-23T12:00:00Z");
    let limited = list_feedback(
        State(pool.clone()),
        AdminUser(admin_id),
        AppQuery(ListFeedbackQuery { limit: Some(2) }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(
        limited.feedback.iter().map(|f| f.id).collect::<Vec<_>>(),
        [2, 3]
    );
    for limit in [0, -1] {
        assert!(matches!(
            list_feedback(
                State(pool.clone()),
                AdminUser(admin_id),
                AppQuery(ListFeedbackQuery { limit: Some(limit) })
            )
            .await,
            Err(AppError::BadRequest(_))
        ));
    }
    sqlx::query("DELETE FROM feedback")
        .execute(&pool)
        .await
        .unwrap();
    let empty = list_feedback(
        State(pool),
        AdminUser(admin_id),
        AppQuery(ListFeedbackQuery::default()),
    )
    .await
    .unwrap()
    .0;
    assert!(empty.feedback.is_empty());
}

#[tokio::test]
async fn feedback_list_rejects_non_admin_and_invalid_limits_over_http() {
    use crate::test_support::test_admin_user;
    use axum::{
        Router,
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt;
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let admin_id = test_admin_user(&pool).await;
    let app = Router::new()
        .route("/api/feedback", get(list_feedback).post(submit_feedback))
        .with_state(pool.clone());
    let pool_for_post = pool;
    let request = |query: &str, user: Option<i64>| {
        let mut builder = Request::builder().uri(format!("/api/feedback{query}"));
        if let Some(id) = user {
            let token = crate::auth::generate_token(id).unwrap();
            builder = builder.header(
                "cookie",
                format!("{}={token}", crate::auth_cookie::COOKIE_NAME),
            );
        }
        builder.body(axum::body::Body::empty()).unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(request("", None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(request("", Some(user_id)))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    for query in [
        "?limit=0",
        "?limit=-1",
        "?limit=1.5",
        "?limit=abc",
        "?limit=9223372036854775808",
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request(query, Some(admin_id)))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        app.clone()
            .oneshot(request("?limit=1", Some(admin_id)))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    // Same path, different method: any signed-in user may submit.
    let sentence_id: i64 = sqlx::query_scalar("SELECT id FROM sentences LIMIT 1")
        .fetch_one(&pool_for_post)
        .await
        .unwrap();
    let token = crate::auth::generate_token(user_id).unwrap();
    let post = Request::builder()
        .method("POST")
        .uri("/api/feedback")
        .header("content-type", "application/json")
        .header(
            "cookie",
            format!("{}={token}", crate::auth_cookie::COOKIE_NAME),
        )
        .body(axum::body::Body::from(
            serde_json::json!({ "sentence_id": sentence_id, "body": "Typo" }).to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.oneshot(post).await.unwrap().status(),
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn admins_delete_feedback_once_and_others_are_forbidden() {
    use crate::test_support::test_admin_user;
    use axum::{
        Router,
        http::{Request, StatusCode},
        routing::delete,
    };
    use tower::ServiceExt;
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let admin_id = test_admin_user(&pool).await;
    let sentence_id: i64 = sqlx::query_scalar("SELECT id FROM sentences LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (_, axum::Json(created)) = submit_feedback(
        State(pool.clone()),
        AuthUser(user_id),
        AppJson(FeedbackRequest {
            sentence_id,
            body: "Typo".into(),
        }),
    )
    .await
    .unwrap();
    let app = Router::new()
        .route("/api/feedback/{feedback_id}", delete(delete_feedback))
        .with_state(pool.clone());
    let request = |user: i64| {
        let token = crate::auth::generate_token(user).unwrap();
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/feedback/{}", created.id))
            .header(
                "cookie",
                format!("{}={token}", crate::auth_cookie::COOKIE_NAME),
            )
            .body(axum::body::Body::empty())
            .unwrap()
    };
    // Even the author cannot delete; triage is an admin action.
    assert_eq!(
        app.clone()
            .oneshot(request(user_id))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.clone()
            .oneshot(request(admin_id))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.oneshot(request(admin_id)).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM feedback")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn feedback_bodies_are_trimmed_and_bounded() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let sentence_id: i64 = sqlx::query_scalar("SELECT id FROM sentences LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let submit = |body: String| {
        let pool = pool.clone();
        async move {
            submit_feedback(
                State(pool),
                AuthUser(user_id),
                AppJson(FeedbackRequest { sentence_id, body }),
            )
            .await
        }
    };
    for body in [
        String::new(),
        " \n\t ".into(),
        "x".repeat(FEEDBACK_MAX_CHARS + 1),
    ] {
        assert!(matches!(submit(body).await, Err(AppError::BadRequest(_))));
    }
    // The limit counts characters, not bytes.
    assert!(submit("가".repeat(FEEDBACK_MAX_CHARS)).await.is_ok());
    let (_, axum::Json(created)) = submit("  Typo here \n".into()).await.unwrap();
    let stored: String = sqlx::query_scalar("SELECT body FROM feedback WHERE id = ?")
        .bind(created.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, "Typo here");
}
