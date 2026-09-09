-- Drops `passkeys.credential_id`, the last column beyond `public_key` that
-- distinguished one stored passkey from another. It served two jobs:
-- `login_finish`'s narrowing key (a cheap byte-equality scan before the one
-- real signature verification it used to run) and `add_passkey_start`'s
-- already-registered-device exclusion list. Without it, both callers now
-- honestly do what README.md's Auth section has always claimed they do:
-- `login_finish` genuinely brute-forces every stored public key's signature
-- (see its own comments - up to N real ECDSA verifies per login attempt,
-- not a shortcut), and `add_passkey_start` can no longer exclude an
-- authenticator already registered to the account, since there's nothing
-- left to exclude by. See the session this migration was written in for
-- why that gap between the README and the code existed in the first place
-- (short version: it never matched anything actually committed, on this
-- branch or the old `demo` branch either - `credential_id` has been its
-- own column since the passkeys table's very first commit) and the
-- decision to close it by changing the code rather than the README.
--
-- Same SQLite limitation as every other migration that's reshaped this
-- table applies here too, so this is the same rebuild-and-copy dance.

CREATE TABLE passkeys_new (
    id            INTEGER PRIMARY KEY,
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    public_key    TEXT    NOT NULL,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    last_used_at  TEXT
) STRICT;

INSERT INTO passkeys_new (id, user_id, public_key, created_at, last_used_at)
SELECT id, user_id, public_key, created_at, last_used_at
FROM passkeys;

DROP TABLE passkeys;
ALTER TABLE passkeys_new RENAME TO passkeys;

CREATE INDEX IF NOT EXISTS idx_passkeys_user_id ON passkeys(user_id);
