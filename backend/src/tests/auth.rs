use super::*;

// --- parse_jwt_ttl_seconds -----------------------------------------------

#[test]
fn ttl_unset_is_none() {
    assert_eq!(parse_jwt_ttl_seconds(None), None);
}

#[test]
fn ttl_zero_is_none() {
    assert_eq!(parse_jwt_ttl_seconds(Some("0".to_string())), None);
}

#[test]
fn ttl_positive_is_some() {
    assert_eq!(parse_jwt_ttl_seconds(Some("3600".to_string())), Some(3600));
}

#[test]
fn ttl_tolerates_surrounding_whitespace() {
    assert_eq!(parse_jwt_ttl_seconds(Some("  60  ".to_string())), Some(60));
}

#[test]
#[should_panic(expected = "must be a non-negative integer")]
fn ttl_negative_panics() {
    parse_jwt_ttl_seconds(Some("-1".to_string()));
}

#[test]
#[should_panic(expected = "must be a non-negative integer")]
fn ttl_non_numeric_panics() {
    parse_jwt_ttl_seconds(Some("soon".to_string()));
}

// --- encode_token / decode_token -----------------------------------------

#[test]
fn round_trips_a_token() {
    let token = encode_token(42, "test-secret", None).unwrap();
    let claims = decode_token(&token, "test-secret").unwrap();
    assert_eq!(claims.sub, 42);
    assert_eq!(claims.exp, None);
}

// No separate "token without exp is accepted" / "future exp is
// accepted" cases: both are the same round_trips_a_token scenario
// (encode then decode succeeds) with different data, and neither adds
// coverage beyond what expired_token_is_rejected already implies (if
// exp weren't wired up at all, that test would fail).

#[test]
fn expired_token_is_rejected() {
    // Build a token with exp already in the past directly, rather than
    // via encode_token (which always computes exp relative to now).
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    // Well beyond jsonwebtoken's default 60s leeway, so this isn't a
    // borderline case.
    let claims = Claims {
        sub: 1,
        exp: Some((Utc::now() - Duration::seconds(3600)).timestamp()),
    };
    let mut header = Header::default();
    header.alg = Algorithm::HS256;
    let token = encode(&header, &claims, &EncodingKey::from_secret(b"s")).unwrap();

    assert!(matches!(
        decode_token(&token, "s"),
        Err(AppError::Unauthenticated)
    ));
}

#[test]
fn wrong_secret_is_rejected() {
    let token = encode_token(1, "right-secret", None).unwrap();
    assert!(matches!(
        decode_token(&token, "wrong-secret"),
        Err(AppError::Unauthenticated)
    ));
}

// No "garbage token is rejected" test: that exercises jsonwebtoken's
// own parse-failure path, not any logic of ours.

// --- signup validation ---------------------------------------------------

fn signup_state(pool: SqlitePool) -> crate::passkey::AppState {
    let mut state = crate::passkey::AppState::new(pool);
    state.signups_enabled = true;
    state
}

async fn signup_as(
    state: &crate::passkey::AppState,
    username: &str,
    password: &str,
) -> Result<(StatusCode, Response), AppError> {
    signup(
        State(state.clone()),
        AppJson(SignupRequest {
            username: username.to_string(),
            password: password.to_string(),
        }),
    )
    .await
}

async fn user_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn signup_rejects_empty_or_oversized_input_without_creating_a_user() {
    let pool = crate::test_support::test_pool().await;
    let state = signup_state(pool.clone());
    let long_username = "u".repeat(USERNAME_MAX_CHARS + 1);
    let long_password = "p".repeat(PASSWORD_MAX_CHARS + 1);

    for (username, password) in [
        ("", "password"),
        ("   ", "password"),
        ("\t\n", "password"),
        ("alice", ""),
        ("alice", "   "),
        ("  ", ""),
        (long_username.as_str(), "password"),
        ("alice", long_password.as_str()),
    ] {
        let error = signup_as(&state, username, password).await.unwrap_err();
        assert!(
            matches!(error, AppError::BadRequest(_)),
            "{username:?}/{password:?} gave {error:?}"
        );
        assert_eq!(error.into_response().status(), StatusCode::BAD_REQUEST);
    }
    assert_eq!(user_count(&pool).await, 0);
}

#[tokio::test]
async fn signup_accepts_trimmed_input_up_to_the_limits() {
    let pool = crate::test_support::test_pool().await;
    let state = signup_state(pool.clone());

    let (status, _) = signup_as(&state, "  alice  ", "  secret  ").await.unwrap();
    assert_eq!(status, StatusCode::CREATED);
    let (username, hash): (String, String) =
        sqlx::query_as("SELECT username, password_hash FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(username, "alice");
    // The stored hash is of the trimmed password, which login also trims.
    let parsed = PasswordHash::new(&hash).unwrap();
    assert!(
        Argon2::default()
            .verify_password(b"secret", &parsed)
            .is_ok()
    );

    // Exactly at the limits (multi-byte characters count once each).
    let max_username = "한".repeat(USERNAME_MAX_CHARS);
    let max_password = "비".repeat(PASSWORD_MAX_CHARS);
    let (status, _) = signup_as(&state, &max_username, &max_password)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(user_count(&pool).await, 2);
}

#[test]
fn signup_schema_publishes_the_enforced_limits() {
    use utoipa::OpenApi;
    let spec = serde_json::to_value(crate::openapi::ApiDoc::openapi()).unwrap();
    let props = &spec["components"]["schemas"]["SignupRequest"]["properties"];
    assert_eq!(props["username"]["minLength"], 1);
    assert_eq!(props["username"]["maxLength"], USERNAME_MAX_CHARS);
    assert_eq!(props["password"]["minLength"], 1);
    assert_eq!(props["password"]["maxLength"], PASSWORD_MAX_CHARS);
    let login = &spec["components"]["schemas"]["LoginRequest"]["properties"];
    assert!(login["username"].get("minLength").is_none());
    assert!(login["password"].get("maxLength").is_none());
    let responses = &spec["paths"]["/api/auth/signup"]["post"]["responses"];
    assert!(responses.get("400").is_some());
}
