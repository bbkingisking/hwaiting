-- Cards table (vocabulary words)
CREATE TABLE IF NOT EXISTS cards (
    id             INTEGER PRIMARY KEY,
    krdict_id      INTEGER,
    word           TEXT    NOT NULL,
    definition     TEXT,
    pos            TEXT,
    origin_type    TEXT,
    hanja          TEXT,
    hanja_eum      TEXT,
    grade          TEXT,
    frequency_rank INTEGER,
    audio_path     TEXT,
    created_at     TEXT    NOT NULL DEFAULT (datetime('now'))
) STRICT;

-- Card translations table
CREATE TABLE IF NOT EXISTS card_translations (
    id           INTEGER PRIMARY KEY,
    card_id      INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    language_tag TEXT    NOT NULL DEFAULT 'en',
    trans_word   TEXT    NOT NULL,
    trans_dfn    TEXT
) STRICT;

-- Custom card metadata (marks user-created cards)
CREATE TABLE IF NOT EXISTS custom_card_metadata (
    card_id    INTEGER PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT    NOT NULL DEFAULT (datetime('now'))
) STRICT;

-- Indexes
CREATE INDEX IF NOT EXISTS idx_cards_word ON cards(word);
CREATE INDEX IF NOT EXISTS idx_cards_krdict_id ON cards(krdict_id);
CREATE INDEX IF NOT EXISTS idx_cards_frequency ON cards(frequency_rank);
CREATE INDEX IF NOT EXISTS idx_card_translations_card_id ON card_translations(card_id);
CREATE INDEX IF NOT EXISTS idx_card_translations_language ON card_translations(language_tag);
CREATE INDEX IF NOT EXISTS idx_custom_card_metadata_user ON custom_card_metadata(user_id);