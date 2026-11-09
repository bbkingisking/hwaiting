use super::*;
use std::sync::Mutex;

// credential_file_name is pure - safe to run alongside anything else.
#[test]
fn credential_file_name_lowercases_and_dashes() {
    assert_eq!(
        credential_file_name("HWAITING_JWT_SECRET"),
        "hwaiting-jwt-secret"
    );
    assert_eq!(
        credential_file_name("HWAITING_ADMIN_USERNAME"),
        "hwaiting-admin-username"
    );
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
