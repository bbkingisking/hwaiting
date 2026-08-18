-- Conjugation/inflection tables for 동사/형용사 cards, generated via koparadigm
-- (https://pypi.org/project/koparadigm/) plus hand-verified corrections for its
-- known-wrong cases (ㄹ-final stems, the 그렇다/이렇다/저렇다/어떻다 mis-tag, and
-- ambiguous multi-class stems like 굽다). See scripts/populate_inflections.py
-- for the generator; this migration only creates the schema and seeds the
-- fixed catalog of inflection forms, mirroring the grammar_patterns pattern
-- (20240101000010/12/14/17): add a new inflection later by inserting a row
-- into inflection_forms, no code changes needed.
--
-- Deliberately independent of the existing tenses/speech_levels lookup
-- tables (20240101000011) even though roughly half these rows line up with
-- a tense+speech_level pair — the other half (connectives, nominalizer,
-- imperative, propositive, interrogative) don't have a tense at all, so
-- forcing everything through those FKs would leave them mostly null for no
-- current benefit. label_ko reuses that table's register terminology
-- (해요체/하십시오체/해체/해라체) for consistency, without a structural FK.

CREATE TABLE IF NOT EXISTS inflection_categories (
    id         INTEGER PRIMARY KEY,
    slug       TEXT    NOT NULL UNIQUE,
    label_en   TEXT    NOT NULL,
    label_ko   TEXT    NOT NULL,
    sort_order INTEGER NOT NULL
) STRICT;

INSERT INTO inflection_categories (slug, label_en, label_ko, sort_order) VALUES
    ('present',       'Present',              '현재',     1),
    ('past',          'Past',                 '과거',     2),
    ('future',        'Future / presumptive', '미래·추측', 3),
    ('command',       'Command & suggestion', '명령·청유', 4),
    ('connectives',   'Connectives',          '연결형',    5),
    ('interrogative', 'Interrogative',        '의문형',    6);

-- Catalog of every inflected form we generate for a card.
CREATE TABLE IF NOT EXISTS inflection_forms (
    id          INTEGER PRIMARY KEY,
    slug        TEXT    NOT NULL UNIQUE,
    label_en    TEXT    NOT NULL,
    label_ko    TEXT    NOT NULL,
    ending_ko   TEXT    NOT NULL,
    category_id INTEGER NOT NULL REFERENCES inflection_categories(id),
    verb_only   INTEGER NOT NULL DEFAULT 0, -- 1 = doesn't apply to 형용사 cards (propositive, formal imperative, ...)
    sort_order  INTEGER NOT NULL
) STRICT;

INSERT INTO inflection_forms (slug, label_en, label_ko, ending_ko, category_id, verb_only, sort_order) VALUES
    ('dictionary_form',      'Dictionary form',                        '사전형',          '-다',            (SELECT id FROM inflection_categories WHERE slug = 'present'), 0, 1),
    ('present_haeyo',        'Present, polite informal (해요체)',       '현재, 해요체',     '-아/어/여요',     (SELECT id FROM inflection_categories WHERE slug = 'present'), 0, 2),
    ('present_hae',          'Present, intimate (해체)',                '현재, 해체',       '-아/어/여',       (SELECT id FROM inflection_categories WHERE slug = 'present'), 0, 3),
    ('present_hasipsio',     'Present, formal (하십시오체)',            '현재, 하십시오체',  '-습니다/ㅂ니다',   (SELECT id FROM inflection_categories WHERE slug = 'present'), 0, 4),
    ('present_haera',        'Present, plain (해라체)',                 '현재, 해라체',      '-는다/ㄴ다',       (SELECT id FROM inflection_categories WHERE slug = 'present'), 1, 5),

    ('past_haeyo',           'Past, polite informal (해요체)',          '과거, 해요체',     '-았/었/였어요',   (SELECT id FROM inflection_categories WHERE slug = 'past'), 0, 6),
    ('past_hae',             'Past, intimate (해체)',                   '과거, 해체',       '-았/었/였어',     (SELECT id FROM inflection_categories WHERE slug = 'past'), 0, 7),
    ('past_hasipsio',        'Past, formal (하십시오체)',               '과거, 하십시오체',  '-았/었/였습니다', (SELECT id FROM inflection_categories WHERE slug = 'past'), 0, 8),
    ('past_haera',           'Past, plain (해라체)',                    '과거, 해라체',      '-았/었/였다',     (SELECT id FROM inflection_categories WHERE slug = 'past'), 0, 9),

    ('future_haeyo',         'Future, polite informal (해요체)',        '미래, 해요체',     '-(으)ㄹ 거예요',  (SELECT id FROM inflection_categories WHERE slug = 'future'), 0, 10),
    ('future_hasipsio',      'Future, formal (하십시오체)',             '미래, 하십시오체',  '-(으)ㄹ 겁니다',  (SELECT id FROM inflection_categories WHERE slug = 'future'), 0, 11),
    ('presumptive_haeyo',    'Presumptive, polite informal (해요체)',   '추측·의지, 해요체', '-겠어요',         (SELECT id FROM inflection_categories WHERE slug = 'future'), 0, 12),
    ('presumptive_hasipsio', 'Presumptive, formal (하십시오체)',        '추측·의지, 하십시오체', '-겠습니다',    (SELECT id FROM inflection_categories WHERE slug = 'future'), 0, 13),

    ('request_haeyo',        'Polite request / honorific (해요체)',     '요청·높임, 해요체', '-(으)세요',       (SELECT id FROM inflection_categories WHERE slug = 'command'), 0, 14),
    ('command_hasipsio',     'Formal command (하십시오체)',             '명령, 하십시오체',  '-(으)십시오',     (SELECT id FROM inflection_categories WHERE slug = 'command'), 1, 15),
    ('exclamation_haera',    'Exclamation (해라체)',                    '감탄·명령, 해라체', '-아/어라',        (SELECT id FROM inflection_categories WHERE slug = 'command'), 0, 16),
    ('propositive_haera',    'Propositive, plain (해라체)',             '청유, 해라체',      '-자',             (SELECT id FROM inflection_categories WHERE slug = 'command'), 1, 17),
    ('propositive_hasipsio', 'Propositive, formal (하십시오체)',        '청유, 하십시오체',  '-(으)ㅂ시다',     (SELECT id FROM inflection_categories WHERE slug = 'command'), 1, 18),

    ('connective_and',       'Connective: and / so',                   '연결: 나열',       '-고',            (SELECT id FROM inflection_categories WHERE slug = 'connectives'), 0, 19),
    ('connective_but',       'Connective: but',                        '연결: 대조',       '-지만',          (SELECT id FROM inflection_categories WHERE slug = 'connectives'), 0, 20),
    ('connective_if',        'Connective: if',                         '연결: 조건',       '-(으)면',         (SELECT id FROM inflection_categories WHERE slug = 'connectives'), 0, 21),
    ('connective_cause',     'Connective: because / and then',         '연결: 이유·계기',  '-아/어서',        (SELECT id FROM inflection_categories WHERE slug = 'connectives'), 0, 22),
    ('connective_background','Connective: background (so, by the way)','연결: 배경',       '-는데/(으)ㄴ데',   (SELECT id FROM inflection_categories WHERE slug = 'connectives'), 0, 23),
    ('connective_intent',    'Connective: intend to',                  '연결: 의도',       '-(으)려고',       (SELECT id FROM inflection_categories WHERE slug = 'connectives'), 0, 24),
    ('nominalizer',          'Nominalizer (-ing / noun form)',         '명사형',           '-기',             (SELECT id FROM inflection_categories WHERE slug = 'connectives'), 0, 25),

    ('question_hasipsio',    'Question, formal (하십시오체)',           '의문, 하십시오체',  '-습니까/ㅂ니까',  (SELECT id FROM inflection_categories WHERE slug = 'interrogative'), 0, 26);

-- One row per (card, inflection) that actually resolved. Absence of a row
-- means "not applicable/not available" for that word -- there's no NULL
-- form stored, the row just doesn't exist.
CREATE TABLE IF NOT EXISTS card_inflections (
    id                 INTEGER PRIMARY KEY,
    card_id            INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    inflection_form_id INTEGER NOT NULL REFERENCES inflection_forms(id) ON DELETE CASCADE,
    form               TEXT    NOT NULL,
    is_corrected       INTEGER NOT NULL DEFAULT 0, -- 1 = hand-overridden, not raw koparadigm output
    UNIQUE(card_id, inflection_form_id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_card_inflections_card_id ON card_inflections(card_id);
CREATE INDEX IF NOT EXISTS idx_card_inflections_form_id ON card_inflections(inflection_form_id);
