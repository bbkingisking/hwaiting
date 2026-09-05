-- Adds passkey (WebAuthn) support alongside the existing username/password
-- auth, without touching it. A passkey-created account has neither a
-- username nor a password, so both columns become nullable; a
-- username/password account never has a passkey row, and vice versa -
-- either identifier is optional, but every user has exactly one JWT-issuing
-- path or the other (or, once migrated by hand, both).
--
-- `handle` is the WebAuthn user handle: 16 random bytes, returned by the
-- authenticator on every sign-in so the server can look up the account
-- without any identifier. It is NOT the row id - the id is sequential and
-- guessable, and re-using it as the handle would leak account count/order
-- to anything that can observe raw assertion bytes. Existing rows are
-- backfilled with a fresh random handle same as any new one.
--
-- SQLite can't add a NOT NULL UNIQUE column with no default via ALTER
-- TABLE, and can't drop the NOT NULL off username/password_hash at all, so
-- this rebuilds the table: create the new shape, copy every row across,
-- drop the old table, rename. Safe under this app's connections, which
-- always run with `PRAGMA foreign_keys = OFF` (see db.rs) - the DROP TABLE
-- below does not cascade.

CREATE TABLE users_new (
    id            INTEGER PRIMARY KEY,
    username      TEXT    UNIQUE,
    password_hash TEXT,
    handle        BLOB    NOT NULL UNIQUE,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    is_admin      INTEGER NOT NULL DEFAULT 0
) STRICT;

INSERT INTO users_new (id, username, password_hash, handle, created_at, is_admin)
SELECT id, username, password_hash, randomblob(16), created_at, is_admin FROM users;

DROP TABLE users;
ALTER TABLE users_new RENAME TO users;

CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_handle ON users(handle);

-- One row per registered passkey; a user may have several (one per
-- device/authenticator). `credential` is the full serialized
-- webauthn-rs-core `Credential` (public key, signature counter, backup
-- flags) - the crate's own recommended persistence shape. The counter and
-- backup flags are rewritten on every successful sign-in.
CREATE TABLE IF NOT EXISTS passkeys (
    id            INTEGER PRIMARY KEY,
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credential_id BLOB    NOT NULL UNIQUE,
    credential    TEXT    NOT NULL,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    last_used_at  TEXT
) STRICT;

CREATE INDEX IF NOT EXISTS idx_passkeys_user_id ON passkeys(user_id);
