-- Bootstrap migration. This replaces the entire former migration chain
-- (originally 001 through 048): each of those was a real, working step at
-- the time, but the accumulated history no longer reflects anything a
-- fresh deployment needs to individually replay -- it's one schema, in one
-- final shape. Squashing it is not "rewriting the past": no code checked
-- out at an earlier commit is affected (its own migrations/ is untouched),
-- and no deployment needs the granular steps replayed -- there's exactly
-- one production database, and it already *is* this schema, arrived at by
-- hand during that history. This migration exists so a *new* deployment
-- can reach the same place without replaying 48 steps, several of which
-- corrected earlier ones in the same chain.
--
-- Every CREATE is IF NOT EXISTS and every catalog seed row is INSERT OR
-- IGNORE, deliberately: that makes this migration do two different, safe
-- things depending on what it's run against. Against a genuinely empty
-- database it bootstraps everything from nothing. Against the existing
-- production database (schema and catalog rows already identical to this,
-- content data aside) it's a no-op that only serves to give a fresh
-- `_sqlx_migrations` ledger something correct to record.
--
-- Sample/demo card content is deliberately NOT here -- cards, sentences,
-- targets, and translations are per-deployment content, not schema truth,
-- and migrations 015/016 baking 50 sample cards into the old chain (with
-- data that turned out wrong and incomplete once the conjugation matrix
-- existed) is exactly the mistake this squash is undoing. Sample content
-- now lives in `seed_sample_cards()` (see db.rs), run explicitly and
-- separately from migrations.

-- ---------------------------------------------------------------------
-- Users and auth.
-- ---------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY,
    username      TEXT    NOT NULL UNIQUE,
    password_hash TEXT    NOT NULL,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    is_admin      INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);

CREATE TABLE IF NOT EXISTS invite_codes (
    id              INTEGER PRIMARY KEY,
    code            TEXT    NOT NULL UNIQUE,
    created_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    used_at         TEXT,
    used_by_user_id INTEGER REFERENCES users(id) ON DELETE SET NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_invite_codes_code ON invite_codes(code);
CREATE INDEX IF NOT EXISTS idx_invite_codes_used_by ON invite_codes(used_by_user_id);

CREATE TABLE IF NOT EXISTS users_settings (
    user_id                  INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    show_percentage          INTEGER NOT NULL DEFAULT 1,
    red_threshold            INTEGER NOT NULL DEFAULT 80,
    yellow_threshold         INTEGER NOT NULL DEFAULT 90,
    day_boundary_hour        INTEGER NOT NULL DEFAULT 4,
    auto_progress_on_correct INTEGER NOT NULL DEFAULT 0,
    auto_progress_delay      INTEGER NOT NULL DEFAULT 1500,
    desired_retention        REAL    NOT NULL DEFAULT 0.9,
    daily_new_card_limit     INTEGER NOT NULL DEFAULT 20,
    history_colorized_area   INTEGER NOT NULL DEFAULT 0,
    history_colored_dots     INTEGER NOT NULL DEFAULT 0,
    history_threshold_lines  INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX IF NOT EXISTS idx_users_settings_user_id ON users_settings(user_id);

CREATE TABLE IF NOT EXISTS users_fsrs_parameters (
    user_id    INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    parameters TEXT NOT NULL
) STRICT;

-- ---------------------------------------------------------------------
-- Languages (ISO 639-3 slugs) -- referenced by every bilingual label
-- table below.
-- ---------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS languages (
    id   INTEGER PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE -- ISO 639-3, e.g. 'eng', 'kor'
) STRICT;

-- ---------------------------------------------------------------------
-- Lookup/catalog tables and their per-language labels.
-- ---------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS parts_of_speech (
    id   INTEGER PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE IF NOT EXISTS parts_of_speech_labels (
    id          INTEGER PRIMARY KEY,
    pos_id      INTEGER NOT NULL REFERENCES parts_of_speech(id) ON DELETE CASCADE,
    language_id INTEGER NOT NULL REFERENCES languages(id),
    label       TEXT    NOT NULL,
    UNIQUE(pos_id, language_id)
) STRICT;

CREATE TABLE IF NOT EXISTS origin_types (
    id   INTEGER PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE IF NOT EXISTS origin_types_labels (
    id             INTEGER PRIMARY KEY,
    origin_type_id INTEGER NOT NULL REFERENCES origin_types(id) ON DELETE CASCADE,
    language_id    INTEGER NOT NULL REFERENCES languages(id),
    label          TEXT    NOT NULL,
    UNIQUE(origin_type_id, language_id)
) STRICT;

CREATE TABLE IF NOT EXISTS grades (
    id   INTEGER PRIMARY KEY,
    slug TEXT    NOT NULL UNIQUE,
    rank INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS grades_labels (
    id          INTEGER PRIMARY KEY,
    grade_id    INTEGER NOT NULL REFERENCES grades(id) ON DELETE CASCADE,
    language_id INTEGER NOT NULL REFERENCES languages(id),
    label       TEXT    NOT NULL,
    UNIQUE(grade_id, language_id)
) STRICT;

CREATE TABLE IF NOT EXISTS speech_levels (
    id   INTEGER PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE IF NOT EXISTS speech_levels_labels (
    id              INTEGER PRIMARY KEY,
    speech_level_id INTEGER NOT NULL REFERENCES speech_levels(id) ON DELETE CASCADE,
    language_id     INTEGER NOT NULL REFERENCES languages(id),
    label           TEXT    NOT NULL,
    UNIQUE(speech_level_id, language_id)
) STRICT;

CREATE TABLE IF NOT EXISTS tenses (
    id   INTEGER PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE IF NOT EXISTS tenses_labels (
    id          INTEGER PRIMARY KEY,
    tense_id    INTEGER NOT NULL REFERENCES tenses(id) ON DELETE CASCADE,
    language_id INTEGER NOT NULL REFERENCES languages(id),
    label       TEXT    NOT NULL,
    UNIQUE(tense_id, language_id)
) STRICT;

CREATE TABLE IF NOT EXISTS grammar_patterns (
    id   INTEGER PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE IF NOT EXISTS grammar_patterns_labels (
    id                 INTEGER PRIMARY KEY,
    grammar_pattern_id INTEGER NOT NULL REFERENCES grammar_patterns(id) ON DELETE CASCADE,
    language_id        INTEGER NOT NULL REFERENCES languages(id),
    label              TEXT    NOT NULL,
    UNIQUE(grammar_pattern_id, language_id)
) STRICT;

CREATE TABLE IF NOT EXISTS grammar_patterns_tooltips (
    id                 INTEGER PRIMARY KEY,
    grammar_pattern_id INTEGER NOT NULL REFERENCES grammar_patterns(id) ON DELETE CASCADE,
    language_id        INTEGER NOT NULL REFERENCES languages(id),
    tooltip            TEXT    NOT NULL,
    UNIQUE(grammar_pattern_id, language_id)
) STRICT;

CREATE TABLE IF NOT EXISTS grammar_patterns_endings (
    id                 INTEGER PRIMARY KEY,
    grammar_pattern_id INTEGER NOT NULL REFERENCES grammar_patterns(id) ON DELETE CASCADE,
    ending             TEXT    NOT NULL,
    UNIQUE(grammar_pattern_id, ending)
) STRICT;

-- ---------------------------------------------------------------------
-- Cards and their per-card content.
-- ---------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS cards (
    id                INTEGER PRIMARY KEY,
    krdict_id         INTEGER,
    word              TEXT    NOT NULL,
    definition        TEXT,
    hanja             TEXT,
    frequency_rank    INTEGER,
    audio_path        TEXT,
    created_at        TEXT    NOT NULL DEFAULT (datetime('now')),
    pos_id            INTEGER REFERENCES parts_of_speech(id),
    origin_type_id    INTEGER REFERENCES origin_types(id),
    grade_id          INTEGER REFERENCES grades(id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_cards_word ON cards(word);
CREATE INDEX IF NOT EXISTS idx_cards_krdict_id ON cards(krdict_id);
CREATE INDEX IF NOT EXISTS idx_cards_frequency ON cards(frequency_rank);

CREATE TABLE IF NOT EXISTS custom_card_metadata (
    card_id    INTEGER PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT    NOT NULL DEFAULT (datetime('now'))
) STRICT;

CREATE INDEX IF NOT EXISTS idx_custom_card_metadata_user ON custom_card_metadata(user_id);

CREATE TABLE IF NOT EXISTS cards_translations (
    id          INTEGER PRIMARY KEY,
    card_id     INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    language_id INTEGER NOT NULL REFERENCES languages(id),
    trans_word  TEXT    NOT NULL,
    trans_dfn   TEXT
) STRICT;

CREATE INDEX IF NOT EXISTS idx_cards_translations_card_id ON cards_translations(card_id);
CREATE INDEX IF NOT EXISTS idx_cards_translations_language ON cards_translations(language_id);

CREATE TABLE IF NOT EXISTS cards_comments (
    id         INTEGER PRIMARY KEY,
    card_id    INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body       TEXT    NOT NULL,
    created_at TEXT    NOT NULL DEFAULT (datetime('now'))
) STRICT;

CREATE INDEX IF NOT EXISTS idx_cards_comments_card_id ON cards_comments(card_id);
CREATE INDEX IF NOT EXISTS idx_cards_comments_user_id ON cards_comments(user_id);

-- ---------------------------------------------------------------------
-- Sentences and targets (the substring a card's review actually tests).
-- ---------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS sentences (
    id         INTEGER PRIMARY KEY,
    card_id    INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    text       TEXT    NOT NULL,
    created_at TEXT    NOT NULL DEFAULT (datetime('now'))
) STRICT;

CREATE INDEX IF NOT EXISTS idx_sentences_card_id ON sentences(card_id);

CREATE TABLE IF NOT EXISTS sentences_translations (
    id          INTEGER PRIMARY KEY,
    sentence_id INTEGER NOT NULL REFERENCES sentences(id) ON DELETE CASCADE,
    language_id INTEGER NOT NULL REFERENCES languages(id),
    translation TEXT    NOT NULL,
    UNIQUE(sentence_id, language_id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_sentences_translations_sentence_id ON sentences_translations(sentence_id);

CREATE TABLE IF NOT EXISTS targets (
    sentence_id        INTEGER PRIMARY KEY REFERENCES sentences(id) ON DELETE CASCADE,
    form               TEXT    NOT NULL,
    speech_level_id    INTEGER REFERENCES speech_levels(id),
    tense_id           INTEGER REFERENCES tenses(id),
    is_honorific       INTEGER NOT NULL DEFAULT 0,
    is_humble          INTEGER NOT NULL DEFAULT 0,
    grammar_pattern_id INTEGER REFERENCES grammar_patterns(id)
) STRICT;

CREATE TABLE IF NOT EXISTS targets_alternatives (
    id          INTEGER PRIMARY KEY,
    sentence_id INTEGER NOT NULL REFERENCES targets(sentence_id) ON DELETE CASCADE,
    alt_target  TEXT    NOT NULL,
    UNIQUE(sentence_id, alt_target)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_targets_alternatives_sentence_id ON targets_alternatives(sentence_id);

-- ---------------------------------------------------------------------
-- Review/FSRS state.
-- ---------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS cards_states (
    id          INTEGER PRIMARY KEY,
    card_id     INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    stability   REAL    NOT NULL DEFAULT 0,
    difficulty  REAL    NOT NULL DEFAULT 0,
    last_review TEXT,
    state       TEXT    NOT NULL DEFAULT 'new'
                    CHECK (state IN ('new','learning','review','relearning')),
    UNIQUE(card_id, user_id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_cards_states_user ON cards_states(user_id, card_id);

CREATE TABLE IF NOT EXISTS review_history (
    id             INTEGER PRIMARY KEY,
    card_id        INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    user_id        INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    rating         TEXT    NOT NULL
                       CHECK (rating IN ('again','hard','good','easy')),
    scheduled_days REAL,
    elapsed_days   REAL,
    reviewed_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    stability      REAL,
    difficulty     REAL,
    state          TEXT    CHECK (state IN ('new','learning','review','relearning'))
) STRICT;

CREATE INDEX IF NOT EXISTS idx_review_history_user_card ON review_history(user_id, card_id, reviewed_at);

CREATE TABLE IF NOT EXISTS users_card_flags (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    card_id    INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    suppressed INTEGER NOT NULL DEFAULT 0,
    flagged_at TEXT    DEFAULT (datetime('now')),
    UNIQUE(user_id, card_id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_users_card_flags_card_id ON users_card_flags(card_id);
CREATE INDEX IF NOT EXISTS idx_users_card_flags_suppressed ON users_card_flags(suppressed);
CREATE INDEX IF NOT EXISTS idx_users_card_flags_user_id ON users_card_flags(user_id);

-- ---------------------------------------------------------------------
-- Conjugation matrix: categories -> forms -> per-card generated values.
-- ---------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS conjugation_matrix_categories (
    id         INTEGER PRIMARY KEY,
    slug       TEXT    NOT NULL UNIQUE,
    sort_order INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS conjugation_matrix_categories_labels (
    id          INTEGER PRIMARY KEY,
    category_id INTEGER NOT NULL REFERENCES conjugation_matrix_categories(id) ON DELETE CASCADE,
    language_id INTEGER NOT NULL REFERENCES languages(id),
    label       TEXT    NOT NULL,
    UNIQUE(category_id, language_id)
) STRICT;

CREATE TABLE IF NOT EXISTS conjugation_matrix_forms (
    id                   INTEGER PRIMARY KEY,
    slug                 TEXT    NOT NULL UNIQUE,
    category_id          INTEGER NOT NULL REFERENCES conjugation_matrix_categories(id),
    speech_level_id      INTEGER REFERENCES speech_levels(id),
    tense_id             INTEGER REFERENCES tenses(id),
    ending               TEXT    NOT NULL,
    sort_order           INTEGER NOT NULL,
    restricted_to_pos_id INTEGER REFERENCES parts_of_speech(id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_conjugation_matrix_forms_category ON conjugation_matrix_forms(category_id);

CREATE TABLE IF NOT EXISTS conjugation_matrix_forms_labels (
    id          INTEGER PRIMARY KEY,
    form_id     INTEGER NOT NULL REFERENCES conjugation_matrix_forms(id) ON DELETE CASCADE,
    language_id INTEGER NOT NULL REFERENCES languages(id),
    label       TEXT    NOT NULL,
    UNIQUE(form_id, language_id)
) STRICT;

CREATE TABLE IF NOT EXISTS conjugation_matrix_sources (
    id   INTEGER PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE IF NOT EXISTS conjugation_matrix_cards (
    id        INTEGER PRIMARY KEY,
    card_id   INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    form_id   INTEGER NOT NULL REFERENCES conjugation_matrix_forms(id) ON DELETE CASCADE,
    form      TEXT    NOT NULL,
    source_id INTEGER REFERENCES conjugation_matrix_sources(id),
    added_on  TEXT    DEFAULT (datetime('now')),
    UNIQUE(card_id, form_id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_conjugation_matrix_cards_card_id ON conjugation_matrix_cards(card_id);
CREATE INDEX IF NOT EXISTS idx_conjugation_matrix_cards_form_id ON conjugation_matrix_cards(form_id);
CREATE INDEX IF NOT EXISTS idx_conjugation_matrix_cards_source_id ON conjugation_matrix_cards(source_id);

-- ---------------------------------------------------------------------
-- Catalog seed data. Same for every deployment; INSERT OR IGNORE so this
-- is a no-op wherever it already exists (e.g. production).
-- ---------------------------------------------------------------------

-- languages (2 rows)
INSERT OR IGNORE INTO languages (id, slug) VALUES
    (1, 'eng'),
    (2, 'kor');

-- parts_of_speech (13 rows)
INSERT OR IGNORE INTO parts_of_speech (id, slug) VALUES
    (1, 'verb'),
    (2, 'noun'),
    (3, 'adjective'),
    (4, 'adverb'),
    (5, 'bound-noun'),
    (6, 'pronoun'),
    (7, 'numeral'),
    (8, 'interjection'),
    (9, 'determiner'),
    (10, 'auxiliary-adjective'),
    (11, 'auxiliary-verb'),
    (12, 'particle'),
    (13, 'no-pos');

-- parts_of_speech_labels (26 rows)
INSERT OR IGNORE INTO parts_of_speech_labels (id, pos_id, language_id, label) VALUES
    (1, 1, 1, 'Verb'),
    (2, 2, 1, 'Noun'),
    (3, 3, 1, 'Adjective'),
    (4, 4, 1, 'Adverb'),
    (5, 5, 1, 'Bound Noun'),
    (6, 6, 1, 'Pronoun'),
    (7, 7, 1, 'Numeral'),
    (8, 8, 1, 'Interjection'),
    (9, 9, 1, 'Determiner'),
    (10, 10, 1, 'Auxiliary Adjective'),
    (11, 11, 1, 'Auxiliary Verb'),
    (12, 12, 1, 'Particle'),
    (13, 13, 1, 'No POS'),
    (14, 1, 2, '동사'),
    (15, 2, 2, '명사'),
    (16, 3, 2, '형용사'),
    (17, 4, 2, '부사'),
    (18, 5, 2, '의존 명사'),
    (19, 6, 2, '대명사'),
    (20, 7, 2, '수사'),
    (21, 8, 2, '감탄사'),
    (22, 9, 2, '관형사'),
    (23, 10, 2, '보조 형용사'),
    (24, 11, 2, '보조 동사'),
    (25, 12, 2, '조사'),
    (26, 13, 2, '품사 없음');

-- origin_types (4 rows)
INSERT OR IGNORE INTO origin_types (id, slug) VALUES
    (1, 'native-korean'),
    (2, 'sino-korean'),
    (3, 'loanword'),
    (4, 'hybrid');

-- origin_types_labels (8 rows)
INSERT OR IGNORE INTO origin_types_labels (id, origin_type_id, language_id, label) VALUES
    (1, 1, 1, 'Native Korean'),
    (2, 2, 1, 'Sino-Korean'),
    (3, 3, 1, 'Loanword'),
    (4, 4, 1, 'Hybrid'),
    (5, 1, 2, '고유어'),
    (6, 2, 2, '한자어'),
    (7, 3, 2, '외래어'),
    (8, 4, 2, '혼종어');

-- grades (3 rows)
INSERT OR IGNORE INTO grades (id, slug, rank) VALUES
    (1, 'beginner', 1),
    (2, 'intermediate', 2),
    (3, 'advanced', 3);

-- grades_labels (6 rows)
INSERT OR IGNORE INTO grades_labels (id, grade_id, language_id, label) VALUES
    (1, 1, 1, 'Beginner'),
    (2, 2, 1, 'Intermediate'),
    (3, 3, 1, 'Advanced'),
    (4, 3, 2, '고급'),
    (5, 2, 2, '중급'),
    (6, 1, 2, '초급');

-- speech_levels (6 rows)
INSERT OR IGNORE INTO speech_levels (id, slug) VALUES
    (1, 'hae-che'),
    (2, 'haeyo-che'),
    (3, 'hasipsio-che'),
    (4, 'haera-che'),
    (5, 'hao-che'),
    (6, 'hage-che');

-- speech_levels_labels (12 rows)
INSERT OR IGNORE INTO speech_levels_labels (id, speech_level_id, language_id, label) VALUES
    (1, 1, 1, 'Intimate'),
    (2, 2, 1, 'Polite Informal'),
    (3, 3, 1, 'Formal'),
    (4, 4, 1, 'Plain'),
    (5, 5, 1, 'Semi-Formal'),
    (6, 6, 1, 'Semi-Plain'),
    (7, 1, 2, '해체'),
    (8, 2, 2, '해요체'),
    (9, 3, 2, '하십시오체'),
    (10, 4, 2, '해라체'),
    (11, 5, 2, '하오체'),
    (12, 6, 2, '하게체');

-- tenses (3 rows)
INSERT OR IGNORE INTO tenses (id, slug) VALUES
    (1, 'present'),
    (2, 'past'),
    (3, 'future');

-- tenses_labels (3 rows)
INSERT OR IGNORE INTO tenses_labels (id, tense_id, language_id, label) VALUES
    (1, 1, 1, 'Present'),
    (2, 2, 1, 'Past'),
    (3, 3, 1, 'Future');

-- grammar_patterns (36 rows)
INSERT OR IGNORE INTO grammar_patterns (id, slug) VALUES
    (1, 'realization'),
    (2, 'witnessed-then'),
    (3, 'backdrop'),
    (4, 'mishap'),
    (5, 'prompted'),
    (6, 'shared-context'),
    (7, 'confirm-surprise'),
    (8, 'disbelief'),
    (9, 'reported'),
    (10, 'near-miss'),
    (11, 'regret-completion'),
    (12, 'decision'),
    (13, 'intent-trailing'),
    (14, 'motion-purpose'),
    (15, 'extent'),
    (16, 'escalation'),
    (17, 'for-someone'),
    (18, 'gentle-question'),
    (19, 'indirect-question'),
    (20, 'connective-seo'),
    (21, 'adnominal'),
    (22, 'adverbializer'),
    (23, 'causal-nikka'),
    (24, 'contrast-jiman'),
    (25, 'connective-go'),
    (26, 'conditional-myeon'),
    (27, 'resultative'),
    (28, 'progressive'),
    (29, 'obligation'),
    (30, 'causal-ni'),
    (31, 'carry-along'),
    (32, 'simultaneous'),
    (33, 'negative-imperative'),
    (34, 'ease-difficulty'),
    (35, 'proposal'),
    (36, 'conjecture');

-- grammar_patterns_labels (36 rows)
INSERT OR IGNORE INTO grammar_patterns_labels (id, grammar_pattern_id, language_id, label) VALUES
    (1, 1, 1, 'Realization'),
    (2, 2, 1, 'Witnessed-then'),
    (3, 3, 1, 'Backdrop'),
    (4, 4, 1, 'Mishap'),
    (5, 5, 1, 'Prompted'),
    (6, 6, 1, 'Shared-context'),
    (7, 7, 1, 'Confirm-surprise'),
    (8, 8, 1, 'Disbelief'),
    (9, 9, 1, 'Reported'),
    (10, 10, 1, 'Near-miss'),
    (11, 11, 1, 'Regret/completion'),
    (12, 12, 1, 'Decision'),
    (13, 13, 1, 'Intent (trailing)'),
    (14, 14, 1, 'Motion-purpose'),
    (15, 15, 1, 'Extent'),
    (16, 16, 1, 'Escalation'),
    (17, 17, 1, 'For-someone'),
    (18, 18, 1, 'Gentle question'),
    (19, 19, 1, 'Indirect question'),
    (20, 20, 1, 'Sequential/cause'),
    (21, 21, 1, 'Adnominal'),
    (22, 22, 1, 'Adverbializer'),
    (23, 23, 1, 'Cause/reason'),
    (24, 24, 1, 'Contrast'),
    (25, 25, 1, 'Sequential list'),
    (26, 26, 1, 'Conditional'),
    (27, 27, 1, 'Resultative'),
    (28, 28, 1, 'Progressive/continuous'),
    (29, 29, 1, 'Obligation/necessity'),
    (30, 30, 1, 'Cause/reason (literary)'),
    (31, 31, 1, 'Carry-along'),
    (32, 32, 1, 'Simultaneous action'),
    (33, 33, 1, 'Negative imperative'),
    (34, 34, 1, 'Ease/difficulty'),
    (35, 35, 1, 'Proposal'),
    (36, 36, 1, 'Conjecture');

-- grammar_patterns_tooltips (36 rows)
INSERT OR IGNORE INTO grammar_patterns_tooltips (id, grammar_pattern_id, language_id, tooltip) VALUES
    (1, 1, 1, 'Ending used to express surprise or newly realized information.'),
    (2, 2, 1, 'Reports something the speaker personally observed happening, followed by a related or contrasting result.'),
    (3, 3, 1, 'Sets up context or a soft contrast rather than a hard "but" — often left dangling to imply something unsaid.'),
    (4, 4, 1, 'Cites a cause that led to something negative or wasted, not neutral causation.'),
    (5, 5, 1, 'Cites something the speaker noticed that triggered their own next action; can''t be used for someone else''s reaction.'),
    (6, 6, 1, 'Gives a reason assuming the listener didn''t know it yet or will find it relevant.'),
    (7, 7, 1, 'Checks something the speaker heard against the listener, often with a note of surprise.'),
    (8, 8, 1, 'Reacts to reported information with shock or disbelief, more emotional than a plain quote.'),
    (9, 9, 1, 'Compressed hearsay — "I heard X" — collapsed from the fuller quotative form.'),
    (10, 10, 1, 'Something almost happened but didn''t; easy to mistranslate as if it did happen.'),
    (11, 11, 1, 'Marks an action as fully finished, often with a tinge of regret or relief.'),
    (12, 12, 1, 'Marks a decision being made or already settled, not just an intention in progress.'),
    (13, 13, 1, 'States a goal or plan as the reason for something, often left unfinished at the end of the sentence.'),
    (14, 14, 1, '"In order to," restricted to verbs of literal movement like 가다 and 오다.'),
    (15, 15, 1, '"So that" or "to the point of," implying more effort or a firmer limit than a plain purpose ending.'),
    (16, 16, 1, 'The more..., the more... — pairs with itself across two clauses.'),
    (17, 17, 1, 'Marks the action as done for or toward someone else''s benefit, even when the English translation doesn''t make that explicit.'),
    (18, 18, 1, 'A softer, more considerate question ending than -아/어요, common when asking about someone''s feelings or situation.'),
    (19, 19, 1, 'Turns a question into a noun-like clause embedded in a larger sentence — needs a following verb like 모르다, 알다, or 궁금하다.'),
    (20, 20, 1, 'Links two clauses where the first happens before and enables the second — sequence when both are actions, cause when the second is a state or involuntary result.'),
    (21, 21, 1, 'Modifies a following noun; the specific form shifts with tense — past/state, present ongoing, or future/hypothetical — rather than being one fixed shape.'),
    (22, 22, 1, 'Turns an adjective into an adverb or sets a standard/result for the clause that follows — "so as to," "in a way that," often paired with verbs like 하다, 되다, 만들다.'),
    (23, 23, 1, 'States a reason the speaker has just realized or is asserting as justification, and — unlike -아/어서 — can lead into a command, suggestion, or future-tense clause.'),
    (24, 24, 1, 'A plain, general-purpose "but" connecting two clauses, with no cause-effect or backdrop nuance the way -는데/-(으)ㄴ데 carries.'),
    (25, 25, 1, 'Strings together actions or states as a simple list or plain sequence, with no cause-effect relationship implied between them.'),
    (26, 26, 1, '"If" or "when" — sets up a hypothetical, habitual, or general condition for the clause that follows.'),
    (27, 27, 1, 'Leaves the result of an action standing in place, often deliberately arranged for later use or as a consequence — distinct from the completion/finality nuance of -아/어 버리다.'),
    (28, 28, 1, 'Marks an action or state as ongoing or in progress — a fixed aspectual construction, distinct from the plain listing use of -고 that just strings two actions together.'),
    (29, 29, 1, 'Marks the clause as a necessary condition, typically paired with 하다/되다 to mean "must" — easy to confuse with a plain intent or conditional ending.'),
    (30, 30, 1, 'States a reason or circumstance leading into what follows — more literary/narrative than -(으)니까, and typically precedes a command, suggestion, or realization rather than standing as a flat explanation.'),
    (31, 31, 1, 'Marks the result of the action as carried toward (오다) or away from (가다) a reference point — literal transport or figurative continuity — as one fused event, distinct from a loosely sequenced -아/어서 pair of clauses and from the leave-in-place nuance of -아/어 놓다.'),
    (32, 32, 1, 'Marks two actions happening at the same time — "while doing X" — distinct from the plain listing use of -고/-(으)며 with no overlap implied.'),
    (33, 33, 1, 'Combines -지 with 말다 to form a prohibition — "do not do X" — distinct from the plain negation of -지 않다 and from -지 used alone as a bare suggestion or reproach.'),
    (34, 34, 1, 'Attaches -기 to a verb stem before 힘들다/어렵다/쉽다 to say an action is hard or easy to do — a fixed collocation, distinct from using -는 것/게 as a nominalizer instead.'),
    (35, 35, 1, 'A tentative first-person offer or suggestion to do something — "Shall I/we...?" — distinct from a flat statement of intent and from the unrelated conjecture use of the same -(으)ㄹ까 ending with a non-volitional or third-person subject.'),
    (36, 36, 1, 'Wonders aloud about an outcome or state outside the speaker''s control — "I wonder if..." — typically a third-person or non-volitional subject, distinct from the first-person proposal use of the same -(으)ㄹ까 ending.');

-- grammar_patterns_endings (83 rows)
INSERT OR IGNORE INTO grammar_patterns_endings (id, grammar_pattern_id, ending) VALUES
    (1, 21, '-은'),
    (2, 21, '-ㄴ'),
    (3, 21, '-는'),
    (4, 21, '-을'),
    (5, 21, '-ㄹ'),
    (6, 22, '-게'),
    (7, 3, '-는데'),
    (8, 3, '-은데'),
    (9, 3, '-ㄴ데'),
    (10, 31, '-아 오다'),
    (11, 31, '-어 오다'),
    (12, 31, '-아 가다'),
    (13, 31, '-어 가다'),
    (14, 30, '-으니'),
    (15, 30, '-니'),
    (16, 23, '-으니까'),
    (17, 23, '-니까'),
    (18, 26, '-으면'),
    (19, 26, '-면'),
    (20, 7, '-다면서'),
    (21, 7, '-다면서요'),
    (22, 36, '-을까'),
    (23, 36, '-ㄹ까'),
    (24, 25, '-고'),
    (25, 20, '-아서'),
    (26, 20, '-어서'),
    (27, 24, '-지만'),
    (28, 12, '-기로 하다'),
    (29, 8, '-다니'),
    (30, 8, '-다니요'),
    (31, 34, '-기 힘들다'),
    (32, 34, '-기 어렵다'),
    (33, 34, '-기 쉽다'),
    (34, 16, '-을수록'),
    (35, 16, '-ㄹ수록'),
    (36, 15, '-도록'),
    (37, 17, '-아 주다'),
    (38, 17, '-어 주다'),
    (39, 18, '-은가요'),
    (40, 18, '-ㄴ가요'),
    (41, 18, '-나요'),
    (42, 19, '-는지'),
    (43, 19, '-은지'),
    (44, 19, '-ㄴ지'),
    (45, 13, '-으려고'),
    (46, 13, '-으려고요'),
    (47, 13, '-려고'),
    (48, 13, '-려고요'),
    (49, 4, '-느라고'),
    (50, 14, '-으러'),
    (51, 14, '-러'),
    (52, 10, '-을 뻔하다'),
    (53, 10, '-ㄹ 뻔하다'),
    (54, 33, '-지 말다'),
    (55, 29, '-아야 하다'),
    (56, 29, '-아야 되다'),
    (57, 29, '-어야 하다'),
    (58, 29, '-어야 되다'),
    (59, 28, '-고 있다'),
    (60, 5, '-길래'),
    (61, 35, '-을까'),
    (62, 35, '-을까요'),
    (63, 35, '-ㄹ까'),
    (64, 35, '-ㄹ까요'),
    (65, 1, '-네'),
    (66, 1, '-네요'),
    (67, 1, '-군'),
    (68, 1, '-군요'),
    (69, 11, '-아 버리다'),
    (70, 11, '-어 버리다'),
    (71, 9, '-대'),
    (72, 9, '-대요'),
    (73, 9, '-래'),
    (74, 9, '-래요'),
    (75, 27, '-아 놓다'),
    (76, 27, '-어 놓다'),
    (77, 6, '-거든'),
    (78, 6, '-거든요'),
    (79, 32, '-으면서'),
    (80, 32, '-면서'),
    (81, 32, '-으며'),
    (82, 32, '-며'),
    (83, 2, '-더니');

-- conjugation_matrix_categories (7 rows)
INSERT OR IGNORE INTO conjugation_matrix_categories (id, slug, sort_order) VALUES
    (1, 'present', 1),
    (2, 'past', 2),
    (3, 'future', 3),
    (4, 'command', 4),
    (5, 'connectives', 5),
    (6, 'interrogative', 6),
    (7, 'adnominal', 7);

-- conjugation_matrix_categories_labels (14 rows)
INSERT OR IGNORE INTO conjugation_matrix_categories_labels (id, category_id, language_id, label) VALUES
    (1, 1, 1, 'Present'),
    (2, 1, 2, '현재'),
    (3, 2, 1, 'Past'),
    (4, 2, 2, '과거'),
    (5, 3, 1, 'Future'),
    (6, 3, 2, '미래'),
    (7, 4, 1, 'Command & suggestion'),
    (8, 4, 2, '명령·청유'),
    (9, 5, 1, 'Connectives'),
    (10, 5, 2, '연결형'),
    (11, 6, 1, 'Interrogative'),
    (12, 6, 2, '의문형'),
    (13, 7, 1, 'Adnominal'),
    (14, 7, 2, '관형사형');

-- conjugation_matrix_forms (29 rows)
INSERT OR IGNORE INTO conjugation_matrix_forms (id, slug, category_id, speech_level_id, tense_id, ending, sort_order, restricted_to_pos_id) VALUES
    (1, 'present_haeyo', 1, 2, 1, '-아/어/여요', 1, NULL),
    (2, 'present_hae', 1, 1, 1, '-아/어/여', 2, NULL),
    (3, 'present_hasipsio', 1, 3, 1, '-습니다/ㅂ니다', 3, NULL),
    (4, 'present_haera', 1, 4, 1, '-는다/ㄴ다', 4, 1),
    (5, 'past_haeyo', 2, 2, 2, '-았/었/였어요', 5, NULL),
    (6, 'past_hae', 2, 1, 2, '-았/었/였어', 6, NULL),
    (7, 'past_hasipsio', 2, 3, 2, '-았/었/였습니다', 7, NULL),
    (8, 'past_haera', 2, 4, 2, '-았/었/였다', 8, NULL),
    (9, 'future_haeyo', 3, 2, 3, '-(으)ㄹ 거예요', 9, NULL),
    (10, 'future_hasipsio', 3, 3, 3, '-(으)ㄹ 겁니다', 10, NULL),
    (11, 'presumptive_haeyo', 3, 2, NULL, '-겠어요', 11, NULL),
    (12, 'presumptive_hasipsio', 3, 3, NULL, '-겠습니다', 12, NULL),
    (13, 'request_haeyo', 4, 2, NULL, '-(으)세요', 13, NULL),
    (14, 'command_hasipsio', 4, 3, NULL, '-(으)십시오', 14, 1),
    (15, 'exclamation_haera', 4, 4, NULL, '-아/어라', 15, NULL),
    (16, 'propositive_haera', 4, 4, NULL, '-자', 16, 1),
    (17, 'propositive_hasipsio', 4, 3, NULL, '-(으)ㅂ시다', 17, 1),
    (18, 'connective_and', 5, NULL, NULL, '-고', 18, NULL),
    (19, 'connective_but', 5, NULL, NULL, '-지만', 19, NULL),
    (20, 'connective_if', 5, NULL, NULL, '-(으)면', 20, NULL),
    (21, 'connective_cause', 5, NULL, NULL, '-아/어서', 21, NULL),
    (22, 'connective_background', 5, NULL, NULL, '-는데/(으)ㄴ데', 22, NULL),
    (23, 'connective_intent', 5, NULL, NULL, '-(으)려고', 23, NULL),
    (24, 'nominalizer', 5, NULL, NULL, '-기', 24, NULL),
    (25, 'question_hasipsio', 6, 3, NULL, '-습니까/ㅂ니까', 25, NULL),
    (26, 'adnominal_present_verb', 7, NULL, 1, '-는', 26, 1),
    (27, 'adnominal_present_adj', 7, NULL, 1, '-(으)ㄴ', 27, 3),
    (28, 'adnominal_past', 7, NULL, 2, '-(으)ㄴ', 28, 1),
    (29, 'adnominal_future', 7, NULL, 3, '-(으)ㄹ', 29, NULL);

-- conjugation_matrix_forms_labels (32 rows)
INSERT OR IGNORE INTO conjugation_matrix_forms_labels (id, form_id, language_id, label) VALUES
    (1, 9, 1, 'Future'),
    (2, 9, 2, '미래'),
    (3, 10, 1, 'Future'),
    (4, 10, 2, '미래'),
    (5, 11, 1, 'Presumptive'),
    (6, 11, 2, '추측·의지'),
    (7, 12, 1, 'Presumptive'),
    (8, 12, 2, '추측·의지'),
    (9, 13, 1, 'Request / honorific'),
    (10, 13, 2, '요청·높임'),
    (11, 14, 1, 'Command'),
    (12, 14, 2, '명령'),
    (13, 15, 1, 'Exclamation'),
    (14, 15, 2, '감탄'),
    (15, 16, 1, 'Propositive'),
    (16, 16, 2, '청유'),
    (17, 17, 1, 'Propositive'),
    (18, 17, 2, '청유'),
    (19, 18, 1, 'And / so'),
    (20, 18, 2, '나열'),
    (21, 19, 1, 'But'),
    (22, 19, 2, '대조'),
    (23, 20, 1, 'If'),
    (24, 20, 2, '조건'),
    (25, 21, 1, 'Because / and then'),
    (26, 21, 2, '이유·계기'),
    (27, 22, 1, 'Background'),
    (28, 22, 2, '배경'),
    (29, 23, 1, 'Intend to'),
    (30, 23, 2, '의도'),
    (31, 24, 1, 'Nominalizer'),
    (32, 24, 2, '명사형');

-- conjugation_matrix_sources (2 rows)
INSERT OR IGNORE INTO conjugation_matrix_sources (id, slug) VALUES
    (1, 'yongcat'),
    (2, 'koparadigm');
