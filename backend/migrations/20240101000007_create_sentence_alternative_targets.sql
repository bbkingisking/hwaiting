-- Accepted alternative answers for a sentence's target word
CREATE TABLE IF NOT EXISTS sentence_alternative_targets (
    id          INTEGER PRIMARY KEY,
    sentence_id INTEGER NOT NULL REFERENCES sentences(id) ON DELETE CASCADE,
    alt_target  TEXT    NOT NULL,
    UNIQUE(sentence_id, alt_target)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_sentence_alt_targets_sentence_id
    ON sentence_alternative_targets(sentence_id);
