-- Cards table (vocabulary words)
CREATE TABLE IF NOT EXISTS cards (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    word TEXT NOT NULL,
    definition TEXT,
    pos TEXT,
    origin_type TEXT,
    hanja TEXT,
    hanja_eum TEXT,
    grade TEXT,
    frequency_rank INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Card translations table
CREATE TABLE IF NOT EXISTS card_translations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    card_id INTEGER NOT NULL,
    language_tag TEXT NOT NULL,
    trans_word TEXT NOT NULL,
    trans_dfn TEXT,
    FOREIGN KEY (card_id) REFERENCES cards(id) ON DELETE CASCADE,
    UNIQUE(card_id, language_tag)
);

-- Custom card metadata (marks user-created cards)
CREATE TABLE IF NOT EXISTS custom_card_metadata (
    card_id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (card_id) REFERENCES cards(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_cards_word ON cards(word);
CREATE INDEX IF NOT EXISTS idx_cards_frequency ON cards(frequency_rank);
CREATE INDEX IF NOT EXISTS idx_card_translations_card_id ON card_translations(card_id);
CREATE INDEX IF NOT EXISTS idx_card_translations_language ON card_translations(language_tag);
CREATE INDEX IF NOT EXISTS idx_custom_card_metadata_user_id ON custom_card_metadata(user_id);