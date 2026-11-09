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
#[path = "tests/auth_cookie.rs"]
mod tests;
