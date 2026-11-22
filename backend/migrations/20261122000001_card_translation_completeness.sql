-- Completeness is enforced at COMMIT, not by a publication flag. Every
-- card needs a sentence; every sentence needs a target and one translation;
-- each word translation needs a sentence translation in the same language.
-- required_* columns are existence witnesses, NOT preferred review content
-- or language defaults. Repoint them before deleting a referenced row.
--
-- Run in the migration runner's transaction with foreign_keys OFF (table
-- rebuilds), then enable foreign_keys on every writing connection. All cyclic
-- references use deferred NO ACTION so a complete set can be written/deleted
-- in one transaction, while incomplete commits fail.
--
-- Reject incomplete old data rather than silently dropping rows or inventing
-- translations. The guard also verifies all foreign keys before COMMIT,
-- since migration connections deliberately disable FK enforcement.
CREATE TEMP TABLE card_completeness_guard (
    valid INTEGER NOT NULL CONSTRAINT complete_card_data_required CHECK (valid = 1)
);
INSERT INTO card_completeness_guard
SELECT NOT EXISTS (
    SELECT 1 FROM cards c
    WHERE NOT EXISTS (SELECT 1 FROM sentences s WHERE s.card_id = c.id)
    UNION ALL
    SELECT 1 FROM sentences s
    WHERE NOT EXISTS (SELECT 1 FROM targets t WHERE t.sentence_id = s.id)
       OR NOT EXISTS (SELECT 1 FROM sentences_translations st WHERE st.sentence_id = s.id)
    UNION ALL
    SELECT 1 FROM cards_translations ct
    WHERE NOT EXISTS (
        SELECT 1 FROM sentences s JOIN sentences_translations st ON st.sentence_id = s.id
        WHERE s.card_id = ct.card_id AND st.language_id = ct.language_id
    )
    UNION ALL
    SELECT 1 FROM sentences_translations st JOIN sentences s ON s.id = st.sentence_id
    WHERE NOT EXISTS (
        SELECT 1 FROM cards_translations ct
        WHERE ct.card_id = s.card_id AND ct.language_id = st.language_id
    )
);

-- Check orphans before the join above could omit any old row.
INSERT INTO card_completeness_guard
SELECT NOT EXISTS (SELECT 1 FROM pragma_foreign_key_check);

-- White_Space characters are enumerated explicitly: SQLite's default trim()
-- only removes ASCII spaces. Optional translated definitions may be NULL,
-- but any supplied definition must contain non-whitespace text too.
-- Language codes follow the existing ISO 639-3 slug convention; registering
-- a language still does not require any card content in that language.
CREATE TABLE languages_new (
    id INTEGER PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE CHECK (slug GLOB '[a-z][a-z][a-z]')
) STRICT;

CREATE TABLE cards_new (
    id INTEGER PRIMARY KEY,
    krdict_id INTEGER,
    word TEXT NOT NULL CHECK (length(trim(word,
        char(9, 10, 11, 12, 13, 32, 133, 160, 5760, 8192, 8193, 8194, 8195, 8196, 8197, 8198, 8199, 8200, 8201, 8202, 8232, 8233, 8239, 8287, 12288))) > 0),
    definition TEXT,
    hanja TEXT,
    frequency_rank INTEGER,
    audio_path TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    pos_id INTEGER REFERENCES parts_of_speech(id),
    origin_type_id INTEGER REFERENCES origin_types(id),
    grade_id INTEGER REFERENCES grades(id),
    pronunciation TEXT,
    content_revision INTEGER NOT NULL DEFAULT 0,
    required_sentence_id INTEGER NOT NULL,
    FOREIGN KEY (required_sentence_id, id) REFERENCES sentences(id, card_id)
        DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE sentences_new (
    id INTEGER PRIMARY KEY,
    card_id INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    text TEXT NOT NULL CHECK (length(trim(text,
        char(9, 10, 11, 12, 13, 32, 133, 160, 5760, 8192, 8193, 8194, 8195, 8196, 8197, 8198, 8199, 8200, 8201, 8202, 8232, 8233, 8239, 8287, 12288))) > 0),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    required_language_id INTEGER NOT NULL REFERENCES languages(id),
    UNIQUE (id, card_id),
    FOREIGN KEY (id) REFERENCES targets(sentence_id)
        DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (id, card_id, required_language_id)
        REFERENCES sentences_translations(sentence_id, card_id, language_id)
        DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE cards_translations_new (
    id INTEGER PRIMARY KEY,
    card_id INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    language_id INTEGER NOT NULL REFERENCES languages(id),
    trans_word TEXT NOT NULL CHECK (length(trim(trans_word,
        char(9, 10, 11, 12, 13, 32, 133, 160, 5760, 8192, 8193, 8194, 8195, 8196, 8197, 8198, 8199, 8200, 8201, 8202, 8232, 8233, 8239, 8287, 12288))) > 0),
    trans_dfn TEXT CHECK (length(trim(trans_dfn,
        char(9, 10, 11, 12, 13, 32, 133, 160, 5760, 8192, 8193, 8194, 8195, 8196, 8197, 8198, 8199, 8200, 8201, 8202, 8232, 8233, 8239, 8287, 12288))) > 0),
    required_sentence_id INTEGER NOT NULL,
    FOREIGN KEY (required_sentence_id, card_id, language_id)
        REFERENCES sentences_translations(sentence_id, card_id, language_id)
        DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE sentences_translations_new (
    id INTEGER PRIMARY KEY,
    sentence_id INTEGER NOT NULL,
    -- Carries the parent identity so composite FKs can require both the
    -- correct sentence owner and that card's translation in this language.
    card_id INTEGER NOT NULL,
    language_id INTEGER NOT NULL REFERENCES languages(id),
    translation TEXT NOT NULL CHECK (length(trim(translation,
        char(9, 10, 11, 12, 13, 32, 133, 160, 5760, 8192, 8193, 8194, 8195, 8196, 8197, 8198, 8199, 8200, 8201, 8202, 8232, 8233, 8239, 8287, 12288))) > 0),
    UNIQUE (sentence_id, language_id),
    UNIQUE (sentence_id, card_id, language_id),
    FOREIGN KEY (sentence_id, card_id) REFERENCES sentences(id, card_id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (card_id, language_id) REFERENCES cards_translations(card_id, language_id)
        DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE targets_new (
    sentence_id INTEGER PRIMARY KEY REFERENCES sentences(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    form TEXT NOT NULL CHECK (length(trim(form,
        char(9, 10, 11, 12, 13, 32, 133, 160, 5760, 8192, 8193, 8194, 8195, 8196, 8197, 8198, 8199, 8200, 8201, 8202, 8232, 8233, 8239, 8287, 12288))) > 0),
    speech_level_id INTEGER REFERENCES speech_levels(id),
    tense_id INTEGER REFERENCES tenses(id),
    is_honorific INTEGER NOT NULL DEFAULT 0,
    is_humble INTEGER NOT NULL DEFAULT 0,
    grammar_pattern_id INTEGER REFERENCES grammar_patterns(id)
) STRICT;

INSERT INTO languages_new SELECT id, slug FROM languages;

-- Preserve pronunciation from older installations; fresh schemas lack it.
-- SQLite gives a duplicate view column a suffixed name, so the original
-- column wins when present and the NULL supplies it otherwise.
CREATE TEMP VIEW old_cards_with_pronunciation AS SELECT *, NULL AS pronunciation FROM main.cards;

INSERT INTO cards_new
SELECT c.id, c.krdict_id, c.word, c.definition, c.hanja, c.frequency_rank,
       c.audio_path, c.created_at, c.pos_id, c.origin_type_id, c.grade_id,
       c.pronunciation, c.content_revision, (SELECT min(s.id) FROM sentences s WHERE s.card_id = c.id)
FROM old_cards_with_pronunciation c;
DROP VIEW old_cards_with_pronunciation;

INSERT INTO sentences_new
SELECT s.id, s.card_id, s.text, s.created_at,
       (SELECT min(st.language_id) FROM sentences_translations st WHERE st.sentence_id = s.id)
FROM sentences s;

INSERT INTO cards_translations_new
SELECT ct.id, ct.card_id, ct.language_id, ct.trans_word, ct.trans_dfn,
       (SELECT min(s.id) FROM sentences s
        JOIN sentences_translations st ON st.sentence_id = s.id
        WHERE s.card_id = ct.card_id AND st.language_id = ct.language_id)
FROM cards_translations ct;

INSERT INTO sentences_translations_new
SELECT st.id, st.sentence_id, s.card_id, st.language_id, st.translation
FROM sentences_translations st JOIN sentences s ON s.id = st.sentence_id;

INSERT INTO targets_new
SELECT sentence_id, form, speech_level_id, tense_id, is_honorific, is_humble, grammar_pattern_id
FROM targets;

DROP TABLE languages;
DROP TABLE cards;
DROP TABLE sentences;
DROP TABLE cards_translations;
DROP TABLE sentences_translations;
DROP TABLE targets;

ALTER TABLE languages_new RENAME TO languages;
ALTER TABLE cards_new RENAME TO cards;
ALTER TABLE sentences_new RENAME TO sentences;
ALTER TABLE cards_translations_new RENAME TO cards_translations;
ALTER TABLE sentences_translations_new RENAME TO sentences_translations;
ALTER TABLE targets_new RENAME TO targets;

CREATE INDEX idx_cards_word ON cards(word);
CREATE INDEX idx_cards_krdict_id ON cards(krdict_id);
CREATE INDEX idx_cards_frequency ON cards(frequency_rank);
CREATE INDEX idx_cards_required_sentence ON cards(required_sentence_id, id);
CREATE INDEX idx_sentences_card_id ON sentences(card_id);
CREATE INDEX idx_cards_translations_card_id ON cards_translations(card_id);
CREATE INDEX idx_cards_translations_language ON cards_translations(language_id);
CREATE UNIQUE INDEX idx_cards_translations_card_language ON cards_translations(card_id, language_id);
CREATE INDEX idx_cards_translations_required_sentence
    ON cards_translations(required_sentence_id, card_id, language_id);
CREATE INDEX idx_sentences_translations_sentence_id ON sentences_translations(sentence_id);
CREATE INDEX idx_sentences_translations_card_language ON sentences_translations(card_id, language_id);
CREATE INDEX idx_sentences_translations_language ON sentences_translations(language_id);

INSERT INTO card_completeness_guard
SELECT NOT EXISTS (SELECT 1 FROM pragma_foreign_key_check);
DROP TABLE card_completeness_guard;
