-- Grammar patterns lookup table (grammar-ending nuances, e.g. "Realization").
-- To add a new pattern later, just INSERT a row here — no code changes needed:
--   INSERT INTO grammar_patterns (slug, label, tooltip) VALUES ('slug', 'Label', 'Tooltip text.');
CREATE TABLE IF NOT EXISTS grammar_patterns (
    id      INTEGER PRIMARY KEY,
    slug    TEXT NOT NULL UNIQUE,
    label   TEXT NOT NULL,
    tooltip TEXT NOT NULL
) STRICT;

INSERT INTO grammar_patterns (slug, label, tooltip) VALUES
    ('realization', 'Realization', 'Ending used to express surprise or newly realized information.');

ALTER TABLE cards ADD COLUMN grammar_pattern_id INTEGER REFERENCES grammar_patterns(id);
