use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;

/// Used when HWAITING_API_URL isn't set. Matches the port prod listens on;
/// override for local dev or a different deployment.
const DEFAULT_BASE_URL: &str = "http://localhost:15000";

pub fn base_url() -> String {
    env::var("HWAITING_API_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
}

fn home_dir() -> io::Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

/// Holds the JWT saved by `login` and read by every other command. Not the
/// same env vars a browser session would use - this is a standing dummy
/// account whose token just needs to survive between separate process
/// invocations, since each `hwaiting-agent` call starts cold.
fn state_dir() -> io::Result<PathBuf> {
    let base = match env::var_os("XDG_CONFIG_HOME") {
        Some(v) => PathBuf::from(v),
        None => home_dir()?.join(".config"),
    };
    let dir = base.join("hwaiting-agent");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    Ok(dir)
}

fn token_path() -> io::Result<PathBuf> {
    Ok(state_dir()?.join("token"))
}

pub fn save_token(token: &str) -> io::Result<()> {
    let path = token_path()?;
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?
        .write_all(token.as_bytes())
}

pub fn load_token() -> io::Result<String> {
    fs::read_to_string(token_path()?)
}
