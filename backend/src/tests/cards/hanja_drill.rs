use super::*;
use crate::test_support::{test_pool, test_user};

#[tokio::test]
async fn hanja_drill_requires_a_configured_translation_language() {
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt;
    let pool = test_pool().await;
    let user = test_user(&pool).await;
    let token = crate::auth::generate_token(user).unwrap();
    let response = Router::new()
        .route("/api/drills/hanja", get(get_hanja_drill))
        .with_state(pool)
        .oneshot(
            Request::builder()
                .uri("/api/drills/hanja")
                .header("cookie", format!("hwaiting-auth={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(error["code"], "translation_language_not_configured");
}
