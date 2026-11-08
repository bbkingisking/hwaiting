//! JWT transport only: no server-side session storage.

use axum::{
    extract::Request,
    http::{HeaderMap, Method, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use cookie::{Cookie, SameSite, time::Duration};

use crate::{credentials, error::AppError};

pub const COOKIE_NAME: &str = "hwaiting-auth";
pub const CSRF_HEADER: &str = "x-requested-with";
pub const CSRF_VALUE: &str = "hwaiting";

#[derive(Clone, Copy, Debug)]
struct CookiePolicy {
    secure: bool,
    same_site: SameSite,
    partitioned: bool,
}

fn parse_policy(
    secure: &str,
    same_site: &str,
    partitioned: &str,
) -> Result<CookiePolicy, &'static str> {
    let secure = secure
        .parse::<bool>()
        .map_err(|_| "HWAITING_COOKIE_SECURE must be true or false")?;
    let partitioned = partitioned
        .parse::<bool>()
        .map_err(|_| "HWAITING_COOKIE_PARTITIONED must be true or false")?;
    if partitioned && !secure {
        return Err("HWAITING_COOKIE_PARTITIONED=true requires HWAITING_COOKIE_SECURE=true");
    }
    let same_site = match same_site {
        "lax" => SameSite::Lax,
        "strict" => SameSite::Strict,
        "none" if secure => SameSite::None,
        "none" => {
            return Err("HWAITING_COOKIE_SAME_SITE=none requires HWAITING_COOKIE_SECURE=true");
        }
        _ => return Err("HWAITING_COOKIE_SAME_SITE must be lax, strict, or none"),
    };
    Ok(CookiePolicy {
        secure,
        same_site,
        partitioned,
    })
}

fn policy() -> CookiePolicy {
    parse_policy(
        credentials::cookie_secure().as_deref().unwrap_or("false"),
        credentials::cookie_same_site().as_deref().unwrap_or("lax"),
        credentials::cookie_partitioned()
            .as_deref()
            .unwrap_or("false"),
    )
    .unwrap_or_else(|message| panic!("{message}"))
}

pub fn validate_config() {
    policy();
}

fn cookie(value: String, max_age: Duration, policy: CookiePolicy) -> Cookie<'static> {
    Cookie::build((COOKIE_NAME, value))
        .path("/")
        .http_only(true)
        .secure(policy.secure)
        .same_site(policy.same_site)
        .partitioned(policy.partitioned)
        .max_age(max_age)
        .build()
}

pub fn issue(token: String, ttl_seconds: Option<i64>) -> Cookie<'static> {
    let policy = policy();
    // Persist across browser restarts. Without a JWT expiry, use the common
    // browser cap of 400 days for cookie storage; the JWT itself is unchanged.
    let max_age = ttl_seconds
        .map(Duration::seconds)
        .unwrap_or(Duration::days(400));
    cookie(token, max_age, policy)
}

pub fn clear() -> Cookie<'static> {
    cookie(String::new(), Duration::ZERO, policy())
}

pub fn token(headers: &HeaderMap) -> Result<String, AppError> {
    let mut token = None;
    for header in headers.get_all(header::COOKIE) {
        let value = header.to_str().map_err(|_| AppError::InvalidCredentials)?;
        for parsed in Cookie::split_parse(value) {
            let parsed = parsed.map_err(|_| AppError::InvalidCredentials)?;
            if parsed.name() == COOKIE_NAME {
                // Reject ambiguity between different cookie scopes.
                if token.replace(parsed.value().to_owned()).is_some() {
                    return Err(AppError::InvalidCredentials);
                }
            }
        }
    }
    token
        .filter(|value| !value.is_empty())
        .ok_or(AppError::InvalidCredentials)
}

pub async fn protect_api(request: Request, next: Next) -> Result<Response, AppError> {
    // Cross-origin JS must preflight this non-safelisted header. The CORS
    // layer allows exact configured origins only; HTML forms cannot set it.
    // Apply this to login/signup too, preventing login CSRF.
    if !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) && request
        .headers()
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        != Some(CSRF_VALUE)
    {
        return Err(AppError::Forbidden);
    }
    let mut response = next.run(request).await;
    // Unlike Authorization headers, cookies alone do not prohibit shared
    // caching. Keep authenticated API responses out of browser/proxy caches.
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    Ok(response)
}

#[utoipa::path(
    post,
    path = "/api/auth/logout",
    responses((status = 204, description = "Authentication cookie cleared")),
    tag = "auth"
)]
pub async fn logout() -> Response {
    // Also clear expired or invalid cookies; no valid JWT is required.
    (
        axum::http::StatusCode::NO_CONTENT,
        [
            (header::SET_COOKIE, clear().to_string()),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
    )
        .into_response()
}

#[cfg(test)]
mod tests {
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
}
