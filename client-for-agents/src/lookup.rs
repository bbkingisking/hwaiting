//! Looks up a KRDICT dictionary entry by target code, via the `krdict`
//! crate. Unlike the hwaiting API commands in `http`, this talks straight
//! to KRDICT - no hwaiting login or token involved.

use krdict::{Client, TranslationLanguage, ViewParams, ViewQuery};
use serde_json::Value;

use crate::http::AppError;

/// Looks up `word_id` (a KRDICT target code) and dumps the whole entry,
/// with English translations included.
pub fn lookup(word_id: u32) -> Result<Value, AppError> {
    let api_key = std::env::var("KRDICT_API_KEY").map_err(|_| {
        AppError::Message("KRDICT_API_KEY must be set in the environment".to_string())
    })?;

    let client = Client::new(api_key);
    let params = ViewParams {
        query: ViewQuery::TargetCode(word_id),
        translated: vec![TranslationLanguage::English],
    };
    let entry = client
        .view(&params)
        .map_err(|e| AppError::Message(format!("krdict lookup failed: {e}")))?;

    serde_json::to_value(entry)
        .map_err(|e| AppError::Message(format!("could not serialize krdict response: {e}")))
}
