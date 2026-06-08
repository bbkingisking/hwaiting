use std::env;
use std::fs;
use std::path::Path;

/// Read a systemd credential by name.
/// In production, systemd sets CREDENTIALS_DIRECTORY and places decrypted
/// credential files there. Falls back to the given env var for dev builds.
fn read_credential(cred_name: &str, env_fallback: &str) -> String {
    if let Ok(cred_dir) = env::var("CREDENTIALS_DIRECTORY") {
        let path = Path::new(&cred_dir).join(cred_name);
        if path.exists() {
            return fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Failed to read credential {}: {}", cred_name, e))
                .trim_end_matches('\n')
                .to_string();
        }
    }

    env::var(env_fallback)
        .unwrap_or_else(|_| panic!("Neither CREDENTIALS_DIRECTORY/{} nor {} is set", cred_name, env_fallback))
}

pub fn jwt_secret() -> String {
    read_credential("hwaiting-jwt", "JWT_SECRET")
}

pub fn admin_password() -> String {
    read_credential("hwaiting-admin-pw", "ADMIN_PASSWORD")
}
