//! hwaiting-agent is a thin CLI over the hwaiting review API, meant to be driven by an agent (see SKILL.md) rather than a human. Every command prints one JSON value to stdout on success and exits 0; on failure it prints a plain-text message to stderr and exits non-zero. Nothing here interprets card content - that's the skill's job, not this binary's.

mod config;
mod http;
mod lookup;

use clap::{Parser, Subcommand};
use http::AppError;
use serde_json::Value;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "hwaiting-agent", about = "CLI review client for hwaiting", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Log in. This should always succeed. Don't try to debug if it fails, just flag it to the user.
    Login,
    /// Fetch the next due card.
    Review,
    /// Submit a guess for a card and get back the graded result.
    Answer { card_id: String, answer: String },
    /// Record a content-review note against a card.
    Comment { card_id: String, text: String },
    /// Fetch the pos/grade/speech_level/tense/grammar_pattern lookup tables.
    FieldValues,
    /// Look up a card's target word in the official KRDict API.
    Krdict {
        /// The KRDICT target code to look up.
        word_id: u32,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Login => cmd_login(),
        Command::Review => http::review(),
        Command::Answer { card_id, answer } => http::answer(&card_id, &answer),
        Command::Comment { card_id, text } => http::comment(&card_id, &text),
        Command::FieldValues => http::field_values(),
        Command::Krdict { word_id } => lookup::lookup(word_id),
    };

    match result {
        Ok(value) => {
            print_json(&value);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_login() -> Result<Value, AppError> {
    let username = std::env::var("HWAITING_USERNAME").unwrap_or_default();
    let password = std::env::var("HWAITING_PASSWORD").unwrap_or_default();
    if username.is_empty() || password.is_empty() {
        return Err(AppError::Message(
            "HWAITING_USERNAME and HWAITING_PASSWORD must be set in the environment".to_string(),
        ));
    }
    http::login(&username, &password)
}

fn print_json(value: &Value) {
    match serde_json::to_string_pretty(value) {
        Ok(s) => println!("{s}"),
        // Not expected to happen (a Value always serializes), but don't
        // swallow the result if it somehow does.
        Err(_) => println!("{value}"),
    }
}
