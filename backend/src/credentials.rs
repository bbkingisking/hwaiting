use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Read a config value from a systemd credential, falling back to an env
/// var. In production, systemd sets CREDENTIALS_DIRECTORY and places
/// decrypted credential files there (`LoadCredential=`/`SetCredential=`);
/// the env var covers dev builds and any deployment that isn't using
/// systemd credentials at all. Every config option in this app is readable
/// through both - env vars are visible in `/proc/PID/environ`,
/// `systemctl show`, and get inherited by anything the process spawns, so
/// operators who'd rather keep all their config (not just secrets) out of
/// the environment and in one place can.
fn read_config(cred_name: &str, env_name: &str) -> Option<String> {
    if let Ok(cred_dir) = env::var("CREDENTIALS_DIRECTORY") {
        let path = Path::new(&cred_dir).join(cred_name);
        if path.exists() {
            return Some(
                fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("Failed to read credential {}: {}", cred_name, e))
                    .trim_end_matches('\n')
                    .to_string(),
            );
        }
    }

    env::var(env_name).ok()
}

/// Like `read_config`, but for options with no sane default: panics naming
/// both the credential and the env var when neither is set.
fn require_config(cred_name: &str, env_name: &str) -> String {
    read_config(cred_name, env_name).unwrap_or_else(|| {
        panic!(
            "Neither systemd credential '{}' nor env var {} is set",
            cred_name, env_name
        )
    })
}

/// Like `read_config`, but falls back to `default` when neither is set.
fn config_or(cred_name: &str, env_name: &str, default: &str) -> String {
    read_config(cred_name, env_name).unwrap_or_else(|| default.to_string())
}

pub fn jwt_secret() -> String {
    require_config("hwaiting-jwt-secret", "JWT_SECRET")
}

pub fn admin_password() -> String {
    require_config("hwaiting-admin-password", "ADMIN_PASSWORD")
}

/// XDG Base Directory data home: `$XDG_DATA_HOME`, or `$HOME/.local/share`
/// if that's unset. This - not `$XDG_STATE_HOME` - is the right tier for a
/// database of the user's actual cards and review history: XDG_STATE_HOME
/// is for logs/history/UI state the user wouldn't think to back up,
/// XDG_DATA_HOME is for data important and portable enough that they would.
fn xdg_data_home() -> Option<PathBuf> {
    if let Ok(dir) = env::var("XDG_DATA_HOME")
        && !dir.trim().is_empty()
    {
        return Some(PathBuf::from(dir));
    }

    env::var("HOME")
        .ok()
        .filter(|home| !home.trim().is_empty())
        .map(|home| PathBuf::from(home).join(".local/share"))
}

/// Defaults to `<xdg-data-home>/hwaiting/hwaiting.db`, creating that
/// directory if it doesn't exist yet - `create_if_missing` on the sqlite
/// connect options only creates the file, not its parent directory. Unlike
/// `ADMIN_USERNAME`/`HOST`/`PORT`, there's no static default here since the
/// value is derived, not fixed; and if `XDG_DATA_HOME`/`HOME` are both
/// unset too (e.g. some systemd service setups), there's nothing to derive
/// it from, so this still panics rather than picking an arbitrary path.
pub fn database_url() -> String {
    if let Some(value) = read_config("hwaiting-database-url", "DATABASE_URL") {
        return value;
    }

    let dir = xdg_data_home()
        .unwrap_or_else(|| {
            panic!(
                "DATABASE_URL not set, and neither XDG_DATA_HOME nor HOME is set to derive a default location from"
            )
        })
        .join("hwaiting");

    fs::create_dir_all(&dir).unwrap_or_else(|e| {
        panic!("Failed to create default database directory {}: {}", dir.display(), e)
    });

    format!("sqlite://{}/hwaiting.db", dir.display())
}

/// Defaults to "admin" - unlike the values above, there's nothing unsafe
/// about a default here, it's not a secret and not deployment-specific
/// identity like RP_ID/RP_ORIGINS.
pub fn admin_username() -> String {
    config_or("hwaiting-admin-username", "ADMIN_USERNAME", "admin")
}

/// No default, and deliberately not derived from `host()`/`port()` either:
/// those name the internal socket this binary listens on, which in
/// production sits behind a TLS-terminating proxy and is never what the
/// browser sees, so defaulting one from the other would be wrong exactly
/// when it matters most. Unset (see `passkey::build_webauthn`) means
/// passkey sign-in - a genuinely optional feature alongside
/// username/password auth - is off, not misconfigured.
pub fn rp_id() -> Option<String> {
    read_config("hwaiting-rp-id", "RP_ID")
}

pub fn rp_origins() -> Option<String> {
    read_config("hwaiting-rp-origins", "RP_ORIGINS")
}

/// Only consulted on the plain-TCP listener path - defaults here don't
/// affect systemd socket activation or UNIX_SOCKET, which are checked
/// first and don't call this.
pub fn host() -> String {
    config_or("hwaiting-host", "HOST", "127.0.0.1")
}

pub fn port() -> String {
    config_or("hwaiting-port", "PORT", "3000")
}

pub fn unix_socket() -> Option<String> {
    read_config("hwaiting-unix-socket", "UNIX_SOCKET")
}

pub fn jwt_expiry_seconds() -> Option<String> {
    read_config("hwaiting-jwt-expiry-seconds", "JWT_EXPIRY_SECONDS")
}

pub fn static_dir() -> Option<String> {
    read_config("hwaiting-static-dir", "STATIC_DIR")
}

pub fn cors_allowed_origins() -> Option<String> {
    read_config("hwaiting-cors-allowed-origins", "CORS_ALLOWED_ORIGINS")
}
