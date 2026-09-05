-- Trims `passkeys.credential` down to exactly the two fields
-- `authenticate_credential` needs to do anything meaningful under this
-- app's fixed configuration - the credential id (already its own column)
-- and the COSE public key used to verify a signature. Everything else the
-- full webauthn-rs `Credential` struct carries either never gates anything
-- here (`registration_policy`/`user_verified`, since both ceremonies
-- hardcode `UserVerificationPolicy::Required`; `attestation*`, since
-- registration requests `AttestationConveyancePreference::None`;
-- `transports`/`extensions`, written by webauthn-rs but never read back by
-- this app), or is a deliberate trade for a single-user deployment
-- (`counter`, `backup_eligible`/`backup_state` - anti-clone and
-- sync-status tracking this app has no policy that acts on). See the
-- session this migration was written in for the full accounting; `cred_id`
-- inside the old `credential` JSON was also a plain duplicate of the
-- `credential_id` column.
--
-- Same SQLite limitation as the migration that created this table applies
-- to reshaping it, so this is the same rebuild-and-copy dance.

CREATE TABLE passkeys_new (
    id            INTEGER PRIMARY KEY,
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credential_id BLOB    NOT NULL UNIQUE,
    public_key    TEXT    NOT NULL,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    last_used_at  TEXT
) STRICT;

INSERT INTO passkeys_new (id, user_id, credential_id, public_key, created_at, last_used_at)
SELECT id, user_id, credential_id, json_extract(credential, '$.cred'), created_at, last_used_at
FROM passkeys;

DROP TABLE passkeys;
ALTER TABLE passkeys_new RENAME TO passkeys;

CREATE INDEX IF NOT EXISTS idx_passkeys_user_id ON passkeys(user_id);

-- `handle` no longer needs NOT NULL. SQLite treats every NULL in a UNIQUE
-- column as distinct from every other NULL, so a password-only account -
-- which never runs a WebAuthn ceremony and so never needs a `user.id` to
-- hand an authenticator - doesn't need a random 16-byte handle it will
-- never use. New password-only accounts leave this NULL; existing ones
-- keep whatever was backfilled when the column was first added, since
-- there's no way to tell in hindsight which of those handles (if any) are
-- backing a real credential.
CREATE TABLE users_new (
    id            INTEGER PRIMARY KEY,
    username      TEXT    UNIQUE,
    password_hash TEXT,
    handle        BLOB    UNIQUE,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    is_admin      INTEGER NOT NULL DEFAULT 0
) STRICT;

INSERT INTO users_new (id, username, password_hash, handle, created_at, is_admin)
SELECT id, username, password_hash, handle, created_at, is_admin FROM users;

DROP TABLE users;
ALTER TABLE users_new RENAME TO users;

CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_handle ON users(handle);
