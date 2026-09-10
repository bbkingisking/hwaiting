-- Switches `passkeys.public_key` from a webauthn-rs-core `COSEKey` (JSON)
-- to a `webauthn_rp::response::register::StaticState<UncompressedPubKey>`
-- (binary, via its own `Encode`/`Decode`) - the storage format of the
-- pure-Rust `webauthn_rp` crate this app's passkey support was rewritten
-- against, replacing webauthn-rs-core (and with it, the app's only
-- openssl-sys dependency). See the session this migration was written in
-- for why: webauthn-rs-core hard-depends on OpenSSL unconditionally for
-- WebAuthn crypto that this app doesn't use for TLS at all (Caddy
-- terminates TLS in front of this binary), and `webauthn_rp` does the same
-- signature verification with RustCrypto primitives instead.
--
-- The two formats aren't convertible by SQL, or even by a one-off Rust
-- script short of re-deriving each stored key through both crates' parsers
-- - there's no shared intermediate representation to pivot through. So
-- this is a hard cutover, not a reshape: every existing passkey is
-- discarded, and anyone who had one registered must add it again after
-- this deploys (through the same "add a passkey" flow as a first-time
-- registration - their account/password login, if they have one, is
-- unaffected). Same one-time-break shape as any credential-format swap;
-- there is no way to avoid it short of keeping both crates linked in
-- forever just to translate old rows, which defeats the point of the
-- swap.
--
-- Same SQLite ALTER TABLE limitation as every other migration that's
-- reshaped this table, so this is the same rebuild-and-copy dance - except
-- there is nothing worth copying from the old `public_key` column itself.

CREATE TABLE passkeys_new (
    id            INTEGER PRIMARY KEY,
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    public_key    BLOB    NOT NULL,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    last_used_at  TEXT
) STRICT;

DROP TABLE passkeys;
ALTER TABLE passkeys_new RENAME TO passkeys;

CREATE INDEX IF NOT EXISTS idx_passkeys_user_id ON passkeys(user_id);
