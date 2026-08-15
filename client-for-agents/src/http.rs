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

/// Bounds the claim-retry loop in `review`. Each attempt means the server
/// handed back a card that got claimed out from under us between our
/// `live_leases` read and our own `claim_card` call - a genuine race, not
/// the "server doesn't know what's taken" case `live_leases` already
/// prevents. This many losses in a row means either an unreasonable number
/// of agents are running, or something is stuck holding leases past their
/// TTL.
const MAX_CLAIM_ATTEMPTS: usize = 50;

/// Fetches the next due card, passed through verbatim. Sends every card id
/// this process can see leased by a sibling `hwaiting-agent review` (same
/// account, different process - see README) as `exclude`, so the server
/// filters them out itself rather than us discovering collisions one at a
/// time - see `config::live_leases` for why that matters at more than a
/// handful of concurrent agents. The retry loop below only has to cover the
/// narrower race where two processes' `live_leases` reads both miss each
/// other and target the same still-unclaimed card.
pub fn review() -> Result<Value, AppError> {
    let token = require_token()?;

    for _ in 0..MAX_CLAIM_ATTEMPTS {
        let exclude = config::live_leases()
            .map_err(|e| AppError::Message(format!("could not read local leases: {e}")))?;
        let path = if exclude.is_empty() {
            "/api/cards/next".to_string()
        } else {
            let ids = exclude.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
            format!("/api/cards/next?exclude={ids}")
        };
        let envelope = parse(&get(&path, &token)?)?;

        let card_id = envelope
            .get("card")
            .filter(|c| !c.is_null())
            .and_then(|c| c.get("card_id"))
            .and_then(Value::as_i64);
        let Some(card_id) = card_id else {
            // Nothing due at all - not a lease collision, so stop rather
            // than retry.
            return Ok(envelope);
        };

        if config::claim_card(card_id)
            .map_err(|e| AppError::Message(format!("could not claim card {card_id}: {e}")))?
        {
            return Ok(envelope);
        }
        // Someone claimed it between our `live_leases` read and this
        // attempt - loop and re-read leases, which will now include it.
    }

    Err(AppError::Message(format!(
        "gave up after {MAX_CLAIM_ATTEMPTS} attempts - every due card seems to already be leased by another agent"
    )))
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

    // Free the lease `review` took out on this card, now that it's
    // suppressed server-side and will never be handed out again anyway.
    // Best-effort: a parse/IO failure here shouldn't fail an otherwise-
    // successful answer, and a leftover lease just self-expires.
    if let Ok(id) = card_id.parse::<i64>() {
        let _ = config::release_card(id);
    }

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
