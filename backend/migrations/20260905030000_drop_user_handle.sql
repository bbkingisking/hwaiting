-- Removes `users.handle` entirely. It existed to satisfy WebAuthn's
-- requirement that the RP hand the authenticator a `user.id` value before
-- any credential exists, then got persisted and used to resolve the
-- account at sign-in - but `passkeys.credential_id` already does that job
-- for free: it's UNIQUE and sits right next to `user_id`, so
-- `SELECT user_id FROM passkeys WHERE credential_id = ?` resolves the same
-- account in one indexed lookup instead of two (handle -> user_id, then
-- user_id -> passkeys). See the session this migration was written in for
-- the full reasoning, including why this doesn't reopen the
-- sequential-id-leak problem `handle` was originally introduced to avoid
-- (credential_id is already authenticator-generated random data, not a
-- guessable row id).
--
-- `login_finish` now reads the assertion's raw credential id instead of
-- its user handle. `register_start`/`add_passkey_start` still generate a
-- `user.id` value to satisfy the ceremony - they just do it in memory, for
-- the lifetime of that one ceremony, and never persist it.
--
-- Numbered past demo's own `20260905000001`/`20260905020000` migrations
-- (deliberately not shared with this branch - see the session this was
-- written in) so a future merge between the two branches never has two
-- differently-content migrations claiming the same version number.
--
-- Same SQLite ALTER TABLE limitation as prior migrations touching this
-- table (can't drop a UNIQUE column in place), so this is the same
-- rebuild-and-copy dance.

CREATE TABLE users_new (
    id            INTEGER PRIMARY KEY,
    username      TEXT    UNIQUE,
    password_hash TEXT,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    is_admin      INTEGER NOT NULL DEFAULT 0
) STRICT;

INSERT INTO users_new (id, username, password_hash, created_at, is_admin)
SELECT id, username, password_hash, created_at, is_admin FROM users;

DROP TABLE users;
ALTER TABLE users_new RENAME TO users;

CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
