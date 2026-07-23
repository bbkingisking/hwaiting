-- Normalize the remaining enum-like TEXT columns into lookup tables + FKs,
-- mirroring the grammar_patterns pattern from migration 20240101000010.
CREATE TABLE IF NOT EXISTS parts_of_speech (
    id    INTEGER PRIMARY KEY,
    slug  TEXT NOT NULL UNIQUE,  -- the original Korean value, e.g. '동사'
    label TEXT NOT NULL          -- e.g. 'Verb'
) STRICT;
INSERT INTO parts_of_speech (slug, label) VALUES
    ('동사','Verb'), ('명사','Noun'), ('형용사','Adjective'), ('부사','Adverb'),
    ('의존 명사','Bound Noun'), ('대명사','Pronoun'), ('수사','Numeral'),
    ('감탄사','Interjection'), ('관형사','Determiner'),
    ('보조 형용사','Auxiliary Adjective'), ('보조 동사','Auxiliary Verb'),
    ('조사','Particle'), ('품사 없음','No POS');

CREATE TABLE IF NOT EXISTS origin_types (
    id INTEGER PRIMARY KEY, slug TEXT NOT NULL UNIQUE, label TEXT NOT NULL
) STRICT;
INSERT INTO origin_types (slug, label) VALUES
    ('고유어','Native Korean'), ('한자어','Sino-Korean'),
    ('외래어','Loanword'), ('혼종어','Hybrid');

CREATE TABLE IF NOT EXISTS grades (
    id INTEGER PRIMARY KEY, slug TEXT NOT NULL UNIQUE, label TEXT NOT NULL,
    rank INTEGER NOT NULL
) STRICT;
INSERT INTO grades (slug, label, rank) VALUES
    ('초급','Beginner',1), ('중급','Intermediate',2), ('고급','Advanced',3);

CREATE TABLE IF NOT EXISTS speech_levels (
    id INTEGER PRIMARY KEY, slug TEXT NOT NULL UNIQUE, label TEXT NOT NULL
) STRICT;
INSERT INTO speech_levels (slug, label) VALUES
    ('hae-che','Intimate (해체)'), ('haeyo-che','Polite Informal (해요체)'),
    ('hasipsio-che','Formal (하십시오체)'), ('haera-che','Plain (해라체)'),
    ('hao-che','Semi-Formal (하오체)'), ('hage-che','Semi-Plain (하게체)');

CREATE TABLE IF NOT EXISTS tenses (
    id INTEGER PRIMARY KEY, slug TEXT NOT NULL UNIQUE, label TEXT NOT NULL
) STRICT;
INSERT INTO tenses (slug, label) VALUES
    ('present','Present'), ('past','Past'), ('future','Future'),
    ('progressive','Progressive'), ('retrospective','Retrospective');

-- Add new FK columns
ALTER TABLE cards ADD COLUMN pos_id INTEGER REFERENCES parts_of_speech(id);
ALTER TABLE cards ADD COLUMN origin_type_id INTEGER REFERENCES origin_types(id);
ALTER TABLE cards ADD COLUMN grade_id INTEGER REFERENCES grades(id);
ALTER TABLE sentence_inflection_hints ADD COLUMN speech_level_id INTEGER REFERENCES speech_levels(id);
ALTER TABLE sentence_inflection_hints ADD COLUMN tense_id INTEGER REFERENCES tenses(id);

-- Safety net: auto-register any value that somehow isn't in the seed lists above
-- (none exist in current data, but this makes the migration lossless regardless)
INSERT OR IGNORE INTO parts_of_speech (slug, label)
    SELECT DISTINCT pos, pos FROM cards WHERE pos IS NOT NULL;
INSERT OR IGNORE INTO origin_types (slug, label)
    SELECT DISTINCT origin_type, origin_type FROM cards WHERE origin_type IS NOT NULL;
INSERT OR IGNORE INTO grades (slug, label, rank)
    SELECT DISTINCT grade, grade, 99 FROM cards WHERE grade IS NOT NULL;
INSERT OR IGNORE INTO speech_levels (slug, label)
    SELECT DISTINCT speech_level, speech_level FROM sentence_inflection_hints WHERE speech_level IS NOT NULL;
INSERT OR IGNORE INTO tenses (slug, label)
    SELECT DISTINCT tense, tense FROM sentence_inflection_hints WHERE tense IS NOT NULL;

-- Backfill
UPDATE cards SET pos_id = (SELECT id FROM parts_of_speech WHERE slug = cards.pos) WHERE pos IS NOT NULL;
UPDATE cards SET origin_type_id = (SELECT id FROM origin_types WHERE slug = cards.origin_type) WHERE origin_type IS NOT NULL;
UPDATE cards SET grade_id = (SELECT id FROM grades WHERE slug = cards.grade) WHERE grade IS NOT NULL;
UPDATE sentence_inflection_hints SET speech_level_id = (SELECT id FROM speech_levels WHERE slug = sentence_inflection_hints.speech_level) WHERE speech_level IS NOT NULL;
UPDATE sentence_inflection_hints SET tense_id = (SELECT id FROM tenses WHERE slug = sentence_inflection_hints.tense) WHERE tense IS NOT NULL;

-- Drop the old TEXT columns now that everything is backed by FKs
ALTER TABLE cards DROP COLUMN pos;
ALTER TABLE cards DROP COLUMN origin_type;
ALTER TABLE cards DROP COLUMN grade;
ALTER TABLE sentence_inflection_hints DROP COLUMN speech_level;
ALTER TABLE sentence_inflection_hints DROP COLUMN tense;
