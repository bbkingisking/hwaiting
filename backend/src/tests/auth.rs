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
        Err(AppError::InvalidCredentials)
    ));
}

#[test]
fn wrong_secret_is_rejected() {
    let token = encode_token(1, "right-secret", None).unwrap();
    assert!(matches!(
        decode_token(&token, "wrong-secret"),
        Err(AppError::InvalidCredentials)
    ));
}

// No "garbage token is rejected" test: that exercises jsonwebtoken's
// own parse-failure path, not any logic of ours.
