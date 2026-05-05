-- Sentences table (example sentences for cards)
CREATE TABLE IF NOT EXISTS sentences (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    card_id INTEGER NOT NULL,
    text TEXT NOT NULL,
    target TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (card_id) REFERENCES cards(id) ON DELETE CASCADE
);

-- Sentence translations table
CREATE TABLE IF NOT EXISTS sentence_translations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sentence_id INTEGER NOT NULL,
    translation TEXT NOT NULL,
    FOREIGN KEY (sentence_id) REFERENCES sentences(id) ON DELETE CASCADE
);

-- Sentence inflection hints table (grammatical information)
CREATE TABLE IF NOT EXISTS sentence_inflection_hints (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sentence_id INTEGER NOT NULL,
    speech_level TEXT NOT NULL,
    tense TEXT NOT NULL,
    FOREIGN KEY (sentence_id) REFERENCES sentences(id) ON DELETE CASCADE
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_sentences_card_id ON sentences(card_id);
CREATE INDEX IF NOT EXISTS idx_sentence_translations_sentence_id ON sentence_translations(sentence_id);
CREATE INDEX IF NOT EXISTS idx_sentence_inflection_hints_sentence_id ON sentence_inflection_hints(sentence_id);