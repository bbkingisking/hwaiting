-- Sentences table (example sentences for cards)
CREATE TABLE IF NOT EXISTS sentences (
    id         INTEGER PRIMARY KEY,
    card_id    INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    text       TEXT    NOT NULL,
    target     TEXT    NOT NULL,
    created_at TEXT    NOT NULL DEFAULT (datetime('now'))
) STRICT;

-- Sentence inflection hints table (only for 동사 / 형용사)
CREATE TABLE IF NOT EXISTS sentence_inflection_hints (
    sentence_id  INTEGER PRIMARY KEY REFERENCES sentences(id) ON DELETE CASCADE,
    speech_level TEXT NOT NULL,
    tense        TEXT NOT NULL
) STRICT;

-- Sentence translations table
CREATE TABLE IF NOT EXISTS sentence_translations (
    id          INTEGER PRIMARY KEY,
    sentence_id INTEGER NOT NULL REFERENCES sentences(id) ON DELETE CASCADE,
    translation TEXT    NOT NULL,
    UNIQUE(sentence_id)
) STRICT;

-- Indexes
CREATE INDEX IF NOT EXISTS idx_sentences_card_id ON sentences(card_id);
CREATE INDEX IF NOT EXISTS idx_sentence_translations_sentence_id ON sentence_translations(sentence_id);
CREATE INDEX IF NOT EXISTS idx_sentence_inflection_hints_sentence_id ON sentence_inflection_hints(sentence_id);