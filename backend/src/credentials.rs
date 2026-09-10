use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Every backend config option is readable two ways that are kept in
/// lockstep by construction, not by two independently-typed names living
/// next to each other: an env var `HWAITING_KEY_NAME`, or a plain-text file
/// at `$CREDENTIALS_DIRECTORY/hwaiting-key-name` - systemd's
/// `LoadCredential=`/`SetCredential=`/`LoadCredentialEncrypted=` all place
/// the credential's decrypted content there as a plain file, in a
/// per-service tmpfs directory systemd sets up (and points
/// `CREDENTIALS_DIRECTORY` at) before the service starts. The env var wins
/// when both are set: it's the one visible in `ps`/`systemctl show`, so if
/// an operator can see it set, they likely meant it to override whatever's
/// sitting in a credential file they may not remember configuring.
fn read_config(env_name: &str) -> Option<String> {
    if let Ok(value) = env::var(env_name) {
        return Some(value);
    }

    let cred_dir = env::var("CREDENTIALS_DIRECTORY").ok()?;
    let path = Path::new(&cred_dir).join(credential_file_name(env_name));
    if !path.exists() {
        return None;
    }

    Some(
        fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read credential file {}: {}", path.display(), e))
            .trim_end_matches('\n')
            .to_string(),
    )
}

/// `HWAITING_ADMIN_USERNAME` -> `hwaiting-admin-username`: lowercase,
/// underscores to dashes. Purely mechanical so the env var name is the only
/// name callers ever have to write down - there's no second string to keep
/// in sync or typo out of step with the first.
fn credential_file_name(env_name: &str) -> String {
    env_name.to_lowercase().replace('_', "-")
}

/// Like `read_config`, but for options with no sane default: panics naming
/// both the env var and the credential file when neither is set.
fn require_config(env_name: &str) -> String {
    read_config(env_name).unwrap_or_else(|| {
        panic!(
            "Neither env var {} nor systemd credential file '{}' (under $CREDENTIALS_DIRECTORY) is set",
            env_name,
            credential_file_name(env_name)
        )
    })
}

/// Like `read_config`, but falls back to `default` when neither is set.
fn config_or(env_name: &str, default: &str) -> String {
    read_config(env_name).unwrap_or_else(|| default.to_string())
}

pub fn jwt_secret() -> String {
    require_config("HWAITING_JWT_SECRET")
}

pub fn admin_password() -> String {
    require_config("HWAITING_ADMIN_PASSWORD")
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
/// `HWAITING_ADMIN_USERNAME`/`HWAITING_HOST`/`HWAITING_PORT`, there's no
/// static default here since the value is derived, not fixed; and if
/// `XDG_DATA_HOME`/`HOME` are both unset too (e.g. some systemd service
/// setups), there's nothing to derive it from, so this still panics rather
/// than picking an arbitrary path.
pub fn database_url() -> String {
    if let Some(value) = read_config("HWAITING_DATABASE_URL") {
        return value;
    }

    let dir = xdg_data_home()
        .unwrap_or_else(|| {
            panic!(
                "HWAITING_DATABASE_URL not set, and neither XDG_DATA_HOME nor HOME is set to derive a default location from"
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
/// identity like HWAITING_RP_ID/HWAITING_RP_ORIGINS.
pub fn admin_username() -> String {
    config_or("HWAITING_ADMIN_USERNAME", "admin")
}

/// No default, and deliberately not derived from `host()`/`port()` either:
/// those name the internal socket this binary listens on, which in
/// production sits behind a TLS-terminating proxy and is never what the
/// browser sees, so defaulting one from the other would be wrong exactly
/// when it matters most. Unset (see `passkey::build_webauthn`) means
/// passkey sign-in - a genuinely optional feature alongside
/// username/password auth - is off, not misconfigured.
pub fn rp_id() -> Option<String> {
    read_config("HWAITING_RP_ID")
}

pub fn rp_origins() -> Option<String> {
    read_config("HWAITING_RP_ORIGINS")
}

/// Only consulted on the plain-TCP listener path - defaults here don't
/// affect systemd socket activation or HWAITING_UNIX_SOCKET, which are checked
/// first and don't call this.
pub fn host() -> String {
    config_or("HWAITING_HOST", "127.0.0.1")
}

pub fn port() -> String {
    config_or("HWAITING_PORT", "3000")
}

pub fn unix_socket() -> Option<String> {
    read_config("HWAITING_UNIX_SOCKET")
}

pub fn jwt_expiry_seconds() -> Option<String> {
    read_config("HWAITING_JWT_EXPIRY_SECONDS")
}

pub fn static_dir() -> Option<String> {
    read_config("HWAITING_STATIC_DIR")
}

pub fn cors_allowed_origins() -> Option<String> {
    read_config("HWAITING_CORS_ALLOWED_ORIGINS")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // credential_file_name is pure - safe to run alongside anything else.
    #[test]
    fn credential_file_name_lowercases_and_dashes() {
        assert_eq!(credential_file_name("HWAITING_JWT_SECRET"), "hwaiting-jwt-secret");
        assert_eq!(credential_file_name("HWAITING_ADMIN_USERNAME"), "hwaiting-admin-username");
    }

    // --- read_config / require_config / config_or ---------------------------
    //
    // These mutate process-wide environment state (env::set_var/remove_var),
    // so every test below is serialized through this mutex and cleans up
    // after itself via EnvGuard's Drop - two of these running concurrently
    // against the same var names would otherwise flake nondeterministically.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const TEST_VAR: &str = "HWAITING_CREDENTIALS_TEST_ONLY";

    struct EnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        dir: PathBuf,
    }

    impl EnvGuard {
        fn acquire() -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let dir = std::env::temp_dir().join(format!(
                "hwaiting_cred_test_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            // SAFETY: serialized by ENV_LOCK - no other thread in this test
            // binary touches these two var names.
            unsafe {
                env::remove_var(TEST_VAR);
                env::remove_var("CREDENTIALS_DIRECTORY");
            }
            Self { _lock: lock, dir }
        }

        fn set_env(&self, value: &str) {
            unsafe { env::set_var(TEST_VAR, value) };
        }

        fn set_credential_file(&self, content: &str) {
            let path = self.dir.join(credential_file_name(TEST_VAR));
            fs::write(&path, content).unwrap();
            unsafe { env::set_var("CREDENTIALS_DIRECTORY", &self.dir) };
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                env::remove_var(TEST_VAR);
                env::remove_var("CREDENTIALS_DIRECTORY");
            }
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn env_var_only() {
        let guard = EnvGuard::acquire();
        guard.set_env("from-env");
        assert_eq!(read_config(TEST_VAR), Some("from-env".to_string()));
    }

    #[test]
    fn credential_file_only() {
        let guard = EnvGuard::acquire();
        guard.set_credential_file("from-file\n");
        assert_eq!(read_config(TEST_VAR), Some("from-file".to_string()));
    }

    #[test]
    fn env_wins_over_credential_file() {
        let guard = EnvGuard::acquire();
        guard.set_credential_file("from-file");
        guard.set_env("from-env");
        assert_eq!(read_config(TEST_VAR), Some("from-env".to_string()));
    }

    #[test]
    fn neither_set_is_none() {
        let _guard = EnvGuard::acquire();
        assert_eq!(read_config(TEST_VAR), None);
    }

    // No separate tests for config_or/require_config: both are one-line
    // delegations to read_config (Option::unwrap_or_else and
    // Option::unwrap_or_else-with-panic respectively), which is already
    // exercised above for every case these would repeat.
}
