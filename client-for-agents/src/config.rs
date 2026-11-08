use std::env;
use std::fs;
use std::io::{self, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::time::Duration;

/// Used when HWAITING_API_URL isn't set. Matches the port prod listens on;
/// override for local dev or a different deployment.
const DEFAULT_BASE_URL: &str = "http://localhost:15000";

pub fn base_url() -> String {
    env::var("HWAITING_API_URL")
        .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

fn home_dir() -> io::Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

/// Private per-user state shared by successive CLI invocations.
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

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedCookies {
    api_url: String,
    jar: String,
}

pub fn save_cookies(agent: &ureq::Agent) -> io::Result<()> {
    let mut jar = Vec::new();
    agent
        .cookie_jar_lock()
        .save_json(&mut jar)
        .map_err(io::Error::other)?;
    let saved = SavedCookies {
        api_url: base_url(),
        jar: String::from_utf8(jar).map_err(io::Error::other)?,
    };
    let bytes = serde_json::to_vec(&saved).map_err(io::Error::other)?;
    let path = state_dir()?.join("cookies.json");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let temporary = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn load_cookies(agent: &ureq::Agent) -> io::Result<()> {
    let path = state_dir()?.join("cookies.json");
    let saved: SavedCookies =
        serde_json::from_reader(BufReader::new(fs::File::open(path)?)).map_err(io::Error::other)?;
    // Cookies normally span ports. Bind saved CLI credentials to the exact
    // configured backend, so changing HWAITING_API_URL requires a new login.
    if saved.api_url != base_url() {
        return Err(io::Error::other(
            "saved login belongs to a different API URL",
        ));
    }
    agent
        .cookie_jar_lock()
        .load_json(saved.jar.as_bytes())
        .map_err(io::Error::other)
}

pub fn remove_legacy_token() -> io::Result<()> {
    match fs::remove_file(state_dir()?.join("token")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// How long an unreleased lease is honored before it's assumed to belong to
/// a crashed/killed `hwaiting-agent` process rather than one still working
/// the card. Generous relative to how long a review actually takes, since
/// stealing a live lease is far worse than leaving a card idle a bit longer.
const LEASE_TTL: Duration = Duration::from_secs(5 * 60);

fn leases_dir() -> io::Result<PathBuf> {
    let dir = state_dir()?.join("leases");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    Ok(dir)
}

/// Claims `card_id` for this process, so that concurrent `hwaiting-agent`
/// invocations under the same account (see README: multiple agents can run
/// against one dummy user at once) don't both get handed the card from
/// `GET /api/cards/next` before either has answered it - the endpoint is a
/// pure read with no server-side notion of "already shown to someone".
///
/// The claim is a `create_new` file: SQLite isn't involved, but the
/// underlying `open(2)` with `O_CREAT|O_EXCL` is still atomic, so exactly
/// one concurrent caller wins. Returns `Ok(true)` if this process now holds
/// the lease, `Ok(false)` if someone else does.
pub fn claim_card(card_id: i64) -> io::Result<bool> {
    let path = leases_dir()?.join(format!("{card_id}.lease"));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(mut f) => {
            write!(f, "{}", std::process::id())?;
            Ok(true)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let stale = fs::metadata(&path)
                .and_then(|m| m.modified())
                .map(|m| m.elapsed().unwrap_or_default() > LEASE_TTL)
                .unwrap_or(false);
            if stale && fs::remove_file(&path).is_ok() {
                claim_card(card_id)
            } else {
                Ok(false)
            }
        }
        Err(e) => Err(e),
    }
}

/// Lists every card id currently leased by a live (non-stale) sibling
/// process, this process included. Used to build the `exclude` set sent to
/// `GET /api/cards/next`, so the server can filter out every card another
/// agent already has in one round trip, rather than this process
/// discovering collisions one at a time via failed `claim_card` calls - see
/// the module's callers for why that matters once there are enough
/// concurrent agents that a single most-recent `exclude` can't keep up.
/// Best-effort: an unreadable individual entry is skipped rather than
/// failing the whole listing.
pub fn live_leases() -> io::Result<Vec<i64>> {
    let ids = fs::read_dir(leases_dir()?)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .map(|m| m.elapsed().unwrap_or_default() <= LEASE_TTL)
                .unwrap_or(false)
        })
        .filter_map(|entry| entry.path().file_stem()?.to_str()?.parse::<i64>().ok())
        .collect();
    Ok(ids)
}

/// Frees a lease once its card has been graded. Best-effort and idempotent -
/// a missing file (already reclaimed via TTL, or never claimed) isn't an
/// error, since the caller shouldn't fail an otherwise-successful `answer`
/// over cleanup of a marker that's purely local bookkeeping.
pub fn release_card(card_id: i64) -> io::Result<()> {
    match fs::remove_file(leases_dir()?.join(format!("{card_id}.lease"))) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
