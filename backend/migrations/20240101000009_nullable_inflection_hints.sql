-- Make speech_level and tense independently nullable in sentence_inflection_hints.
-- SQLite doesn't support ALTER COLUMN, so recreate the table.
CREATE TABLE sentence_inflection_hints_new (
    sentence_id  INTEGER PRIMARY KEY REFERENCES sentences(id) ON DELETE CASCADE,
    speech_level TEXT,
    tense        TEXT
) STRICT;

INSERT INTO sentence_inflection_hints_new SELECT * FROM sentence_inflection_hints;

DROP TABLE sentence_inflection_hints;

ALTER TABLE sentence_inflection_hints_new RENAME TO sentence_inflection_hints;

CREATE INDEX IF NOT EXISTS idx_sentence_inflection_hints_sentence_id ON sentence_inflection_hints(sentence_id);
