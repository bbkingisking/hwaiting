use super::*;

#[test]
fn cookie_transport_is_http_only_host_scoped_and_matches_expiry() {
    let cookie = cookie(
        "jwt".into(),
        Duration::seconds(3600),
        parse_policy("true", "lax", "false").unwrap(),
    );
    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.secure(), Some(true));
    assert_eq!(cookie.same_site(), Some(SameSite::Lax));
    assert_eq!(cookie.path(), Some("/"));
    assert_eq!(cookie.domain(), None);
    assert_eq!(cookie.max_age(), Some(Duration::seconds(3600)));
}

#[test]
fn demo_cookie_is_partitioned_for_both_login_and_logout() {
    let policy = parse_policy("true", "none", "true").unwrap();
    for (value, age) in [("jwt", Duration::seconds(3600)), ("", Duration::ZERO)] {
        let cookie = cookie(value.into(), age, policy);
        assert_eq!(cookie.partitioned(), Some(true));
        assert_eq!(cookie.secure(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::None));
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.path(), Some("/"));
        assert_eq!(cookie.max_age(), Some(age));
        assert!(cookie.to_string().contains("Partitioned"));
    }
}

#[test]
fn production_policy_keeps_cookies_usable_over_http_and_https() {
    let policy = parse_policy("false", "lax", "false").unwrap();
    let cookie = cookie("jwt".into(), Duration::seconds(3600), policy);
    assert_ne!(cookie.secure(), Some(true));
    assert_ne!(cookie.partitioned(), Some(true));
    assert!(!cookie.to_string().contains("Secure"));
    assert!(!cookie.to_string().contains("Partitioned"));
}

#[test]
fn partitioned_configuration_requires_secure_and_valid_boolean() {
    assert!(parse_policy("false", "none", "true").is_err());
    assert!(parse_policy("false", "lax", "true").is_err());
    assert!(parse_policy("true", "none", "invalid").is_err());
}

#[test]
fn cookie_parser_handles_multiple_headers_and_rejects_duplicate_credentials() {
    let mut headers = HeaderMap::new();
    headers.append(header::COOKIE, "other=value".parse().unwrap());
    headers.append(
        header::COOKIE,
        "hwaiting-auth=jwt; theme=dark".parse().unwrap(),
    );
    assert_eq!(token(&headers).unwrap(), "jwt");
    headers.append(header::COOKIE, "hwaiting-auth=second".parse().unwrap());
    assert!(token(&headers).is_err());
}

#[test]
fn bearer_headers_and_empty_cookies_are_not_credentials() {
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, "Bearer jwt".parse().unwrap());
    assert!(token(&headers).is_err());
    headers.insert(header::COOKIE, "hwaiting-auth=".parse().unwrap());
    assert!(token(&headers).is_err());
}

#[tokio::test]
async fn writes_require_the_csrf_header_but_reads_and_preflight_do_not() {
    use axum::{Router, body::Body, middleware, routing::any};
    use tower::ServiceExt;

    let app = Router::new()
        .route("/", any(|| async { axum::http::StatusCode::OK }))
        .layer(middleware::from_fn(protect_api));
    for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
        let request = Request::builder()
            .method(method.clone())
            .uri("/")
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.clone().oneshot(request).await.unwrap().status(), 403);
        let request = Request::builder()
            .method(method)
            .uri("/")
            .header(CSRF_HEADER, CSRF_VALUE)
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.clone().oneshot(request).await.unwrap().status(), 200);
    }
    for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
        let request = Request::builder()
            .method(method)
            .uri("/")
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.clone().oneshot(request).await.unwrap().status(), 200);
    }
}

#[test]
fn clearing_a_cookie_uses_the_same_scope_and_zero_lifetime() {
    let expired = cookie(
        String::new(),
        Duration::ZERO,
        parse_policy("false", "lax", "false").unwrap(),
    );
    assert_eq!(expired.name(), COOKIE_NAME);
    assert_eq!(expired.path(), Some("/"));
    assert_eq!(expired.domain(), None);
    assert_eq!(expired.value(), "");
    assert_eq!(expired.max_age(), Some(Duration::ZERO));
    assert_ne!(expired.secure(), Some(true));
}
