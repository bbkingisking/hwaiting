//! Talks to the hwaiting review API. Every function here returns the
//! server's JSON response as a `serde_json::Value`, trimmed of fields this
//! review flow has no use for (`review`, `answer`, `grammar_patterns` each
//! note their own) but otherwise untouched - nothing here interprets card
//! content, that's the skill's job, not this binary's.

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

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into()
}

fn get(path: &str, client: &ureq::Agent) -> Result<String, AppError> {
    let url = format!("{}{path}", config::base_url());
    let resp = client.get(&url).call().map_err(unreachable_err)?;
    read_response(client, resp)
}

fn post_json(path: &str, client: &ureq::Agent, body: &Value) -> Result<String, AppError> {
    let url = format!("{}{path}", config::base_url());
    let resp = client
        .post(&url)
        .header("X-Requested-With", "hwaiting")
        .send_json(body)
        .map_err(unreachable_err)?;
    read_response(client, resp)
}

fn put(path: &str, client: &ureq::Agent) -> Result<String, AppError> {
    let url = format!("{}{path}", config::base_url());
    let resp = client
        .put(&url)
        .header("X-Requested-With", "hwaiting")
        .send_empty()
        .map_err(unreachable_err)?;
    read_response(client, resp)
}

fn read_response(
    client: &ureq::Agent,
    response: ureq::http::Response<ureq::Body>,
) -> Result<String, AppError> {
    if response.headers().contains_key("set-cookie") {
        config::save_cookies(client).map_err(|error| {
            AppError::Message(format!("could not save authentication cookies: {error}"))
        })?;
    }
    read_body(response)
}

fn parse(raw: &str) -> Result<Value, AppError> {
    serde_json::from_str(raw).map_err(|e| AppError::Message(format!("malformed response: {e}")))
}

fn authenticated_agent() -> Result<ureq::Agent, AppError> {
    let client = agent();
    config::load_cookies(&client).map_err(|error| {
        AppError::Message(format!("not logged in (run `login` first): {error}"))
    })?;
    if !client
        .cookie_jar_lock()
        .iter()
        .any(|cookie| cookie.name() == "hwaiting-auth")
    {
        return Err(AppError::Message(
            "login has expired (run `login` again)".to_string(),
        ));
    }
    Ok(client)
}

/// Exchanges username/password for a cookie, saved privately for later invocations.
pub fn login(username: &str, password: &str) -> Result<Value, AppError> {
    let client = agent();
    let body = json!({ "username": username, "password": password });
    let raw = post_json("/api/auth/login", &client, &body)?;
    let parsed = parse(&raw)?;
    if !client
        .cookie_jar_lock()
        .iter()
        .any(|cookie| cookie.name() == "hwaiting-auth")
    {
        return Err(AppError::Message(
            "malformed login response: missing authentication cookie".to_string(),
        ));
    }
    config::remove_legacy_token()
        .map_err(|error| AppError::Message(format!("could not remove legacy token: {error}")))?;
    Ok(json!({
        "status": "ok",
        "username": parsed.get("username").cloned().unwrap_or(Value::Null),
        "is_admin": parsed.get("is_admin").cloned().unwrap_or(Value::Null),
    }))
}

/// Fetch one available review entry using the account's saved L1. Local leases
/// keep sibling agents from selecting the same card; the signed attempt token
/// must be returned unchanged when answering.
pub fn review() -> Result<Value, AppError> {
    let client = authenticated_agent()?;
    // A claim can race with another process after the lease snapshot is read.
    // Refresh only in that case, and bound retries under sustained contention.
    const MAX_CLAIM_ATTEMPTS: usize = 50;
    for _ in 0..MAX_CLAIM_ATTEMPTS {
        let leased = config::live_leases()
            .map_err(|e| AppError::Message(format!("could not read local leases: {e}")))?;
        // At most leased.len() entries can be held by known sibling processes.
        // One more is enough to find an available entry without expanding the deck.
        let limit = leased.len().saturating_add(1);
        let envelope = parse(&get(&format!("/api/reviews/queue?limit={limit}"), &client)?)?;
        let mut lost_claim = false;
        for entry in envelope
            .get("cards")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(card_id) = entry["card"]["card_id"].as_i64() else {
                continue;
            };
            if leased.contains(&card_id) {
                continue;
            }
            if config::claim_card(card_id)
                .map_err(|e| AppError::Message(format!("could not claim card {card_id}: {e}")))?
            {
                let mut entry = entry.clone();
                if let Some(review) = entry.get_mut("review").and_then(Value::as_object_mut) {
                    review.remove("hanja_hint_words");
                }
                return Ok(entry);
            }
            lost_claim = true;
        }
        if lost_claim {
            continue;
        }
        // No unleased entry is available; preserve the server's refresh metadata.
        return Ok(json!({"card": null, "review": null,
            "next_due_at": envelope["next_due_at"], "refresh_at": envelope["refresh_at"]}));
    }
    Err(AppError::Message(format!(
        "could not claim a review card after {MAX_CLAIM_ATTEMPTS} attempts; other reviewers kept claiming it first"
    )))
}

/// Submits a guess for the given card and returns the graded result
/// (CheckResponse: correct + the CardReveal fields, minus personalized
/// `hanja_hints`). Then suppresses the card so it never comes up
/// again for this user - right or wrong, the agent has now seen it once, and
/// the point is coverage of the deck rather than mastering it via spaced
/// repetition.
pub fn answer(card_id: &str, guess: &str, attempt_token: &str) -> Result<Value, AppError> {
    let client = authenticated_agent()?;
    let body = json!({ "answer": guess, "attempt_token": attempt_token });
    let mut result = parse(&post_json("/api/reviews/answer", &client, &body)?)?;

    // Personalized review assistance is not needed for this content-review flow.
    if let Some(obj) = result.as_object_mut() {
        obj.remove("hanja_hints");
    }

    put(&format!("/api/cards/{card_id}/suppress"), &client)?;

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
    let client = authenticated_agent()?;
    let body = json!({ "body": text });
    parse(&post_json(
        &format!("/api/cards/{card_id}/comment"),
        &client,
        &body,
    )?)
}

/// Authored grammar catalog for admin editing. Fixed vocabulary labels are
/// client presentation resources; selected grammar content already rides on cards.
pub fn grammar_patterns() -> Result<Value, AppError> {
    let client = authenticated_agent()?;
    parse(&get("/api/admin/grammar-patterns", &client)?)
}
