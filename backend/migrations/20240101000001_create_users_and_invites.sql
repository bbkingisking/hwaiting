-- Users table
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY,
    username      TEXT    NOT NULL UNIQUE,
    password_hash TEXT    NOT NULL,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    is_admin      INTEGER NOT NULL DEFAULT 0
) STRICT;

-- Invite codes table
CREATE TABLE IF NOT EXISTS invite_codes (
    id              INTEGER PRIMARY KEY,
    code            TEXT    NOT NULL UNIQUE,
    created_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    used_at         TEXT,
    used_by_user_id INTEGER REFERENCES users(id) ON DELETE SET NULL
) STRICT;

-- Indexes
CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_invite_codes_code ON invite_codes(code);
CREATE INDEX IF NOT EXISTS idx_invite_codes_used_by ON invite_codes(used_by_user_id);