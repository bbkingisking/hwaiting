//! Talks to the hwaiting review API. Every function here returns the
//! server's JSON response passed through verbatim (as a `serde_json::Value`).
//! Nothing here interprets card content, that's the skill's job, not this
//! binary's.

use serde_json::{Value, json};
use std::fmt;

use crate::config;

/// Wraps a non-2xx response, or a failure to reach the server at all. The
/// server always answers errors with `{"error": "..."}` (see
/// backend/src/error.rs), so that message is surfaced rather than a raw
/// status code.
#[derive(Debug)]
pub enum AppError {
    Api { status: u16, body: String },
    Message(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Api { status, body } => {
                let message = serde_json::from_str::<Value>(body)
                    .ok()
                    .and_then(|v| v.get("error")?.as_str().map(str::to_string))
                    .filter(|s| !s.is_empty());
                match message {
                    Some(m) => write!(f, "{m} (HTTP {status})"),
                    None => write!(f, "HTTP {status}: {body}"),
                }
            }
            AppError::Message(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for AppError {}

fn unreachable_err(e: ureq::Error) -> AppError {
    AppError::Message(format!("could not reach {}: {e}", config::base_url()))
}

fn read_body(mut resp: ureq::http::Response<ureq::Body>) -> Result<String, AppError> {
    let status = resp.status().as_u16();
    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| AppError::Message(e.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(AppError::Api { status, body });
    }
    Ok(body)
}

fn get(path: &str, token: &str) -> Result<String, AppError> {
    let url = format!("{}{path}", config::base_url());
    let resp = ureq::get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(unreachable_err)?;
    read_body(resp)
}

fn post_json(path: &str, token: Option<&str>, body: &Value) -> Result<String, AppError> {
    let url = format!("{}{path}", config::base_url());
    let mut req = ureq::post(&url)
        .config()
        .http_status_as_error(false)
        .build();
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let resp = req.send_json(body).map_err(unreachable_err)?;
    read_body(resp)
}

fn put(path: &str, token: &str) -> Result<String, AppError> {
    let url = format!("{}{path}", config::base_url());
    let resp = ureq::put(&url)
        .header("Authorization", format!("Bearer {token}"))
        .config()
        .http_status_as_error(false)
        .build()
        .send_empty()
        .map_err(unreachable_err)?;
    read_body(resp)
}

fn parse(raw: &str) -> Result<Value, AppError> {
    serde_json::from_str(raw).map_err(|e| AppError::Message(format!("malformed response: {e}")))
}

fn require_token() -> Result<String, AppError> {
    config::load_token()
        .map_err(|e| AppError::Message(format!("not logged in (run `login` first): {e}")))
}

/// Exchanges username/password for a JWT and saves it to disk for every
/// later command to read.
pub fn login(username: &str, password: &str) -> Result<Value, AppError> {
    let body = json!({ "username": username, "password": password });
    let raw = post_json("/api/auth/login", None, &body)?;
    let parsed = parse(&raw)?;

    let token = parsed
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Message("malformed login response: missing token".to_string()))?;
    config::save_token(token)
        .map_err(|e| AppError::Message(format!("logged in but could not save token: {e}")))?;

    // Deliberately doesn't echo the token back - it's already on disk and
    // has no further use in the transcript.
    Ok(json!({
        "status": "ok",
        "username": parsed.get("username").cloned().unwrap_or(Value::Null),
        "is_admin": parsed.get("is_admin").cloned().unwrap_or(Value::Null),
    }))
}

/// Fetches the next due card, passed through verbatim.
pub fn review() -> Result<Value, AppError> {
    let token = require_token()?;
    parse(&get("/api/cards/next", &token)?)
}

/// Submits a guess for the given card and returns the graded result
/// (CheckResponse: correct + the CardReveal fields), passed through
/// verbatim. Then suppresses the card so it never comes up again for this
/// user - right or wrong, the agent has now seen it once, and the point is
/// coverage of the deck rather than mastering it via spaced repetition.
pub fn answer(card_id: &str, guess: &str) -> Result<Value, AppError> {
    let token = require_token()?;
    let body = json!({ "answer": guess });
    let result = parse(&post_json(
        &format!("/api/cards/{card_id}/check"),
        Some(&token),
        &body,
    )?)?;

    put(&format!("/api/cards/{card_id}/suppress"), &token)?;

    Ok(result)
}

/// Records a content-review note against a card server-side.
pub fn comment(card_id: &str, text: &str) -> Result<Value, AppError> {
    let token = require_token()?;
    let body = json!({ "body": text });
    parse(&post_json(
        &format!("/api/cards/{card_id}/comment"),
        Some(&token),
        &body,
    )?)
}

/// Fetches the current pos/origin_type/grade/speech_level/tense/
/// grammar_pattern tables live, so the caller never has to keep a
/// hardcoded copy in sync by hand.
pub fn lookups() -> Result<Value, AppError> {
    let token = require_token()?;
    parse(&get("/api/cards/enum-lookups", &token)?)
}
