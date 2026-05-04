-- =============================================================================
-- Users (no language columns – the language pair is fixed)
-- =============================================================================
CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT    NOT NULL UNIQUE,
    password_hash TEXT    NOT NULL,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    is_admin      INTEGER NOT NULL DEFAULT 0   -- 0/1
) STRICT;

-- =============================================================================
-- Cards (Korean lemma with monolingual Korean definition)
-- =============================================================================
CREATE TABLE cards (
    id             INTEGER PRIMARY KEY,
    krdict_id      INTEGER,                     -- Korean Dictionary entry ID (target_code), nullable for custom cards
    word           TEXT    NOT NULL,            -- the target Korean word
    definition     TEXT,                        -- Korean dictionary definition (sense 1, monolingual)
    pos            TEXT,                        -- part of speech (동사, 명사, 형용사, …)
    origin_type    TEXT,                        -- 고유어 / 한자어 / 외래어 / 혼종어
    hanja          TEXT,                        -- hanja origin if extant
    hanja_eum      TEXT,                        -- Korean pronunciation of hanja (null for 고유어)
    grade          TEXT,                        -- 초급 / 중급 / 고급
    frequency_rank INTEGER,                     -- rank in frequency corpus, null if unmatched
    audio_path     TEXT,
    created_at     TEXT    NOT NULL DEFAULT (datetime('now'))
) STRICT;

-- =============================================================================
-- Custom card metadata (only for user-created cards)
-- =============================================================================
CREATE TABLE custom_card_metadata (
    card_id    INTEGER PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT    NOT NULL DEFAULT (datetime('now'))
) STRICT;

-- =============================================================================
-- Card translations (English definitions / hints)
-- =============================================================================
CREATE TABLE card_translations (
    id           INTEGER PRIMARY KEY,
    card_id      INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    language_tag TEXT    NOT NULL DEFAULT 'en',
    trans_word   TEXT    NOT NULL,              -- short English equivalent (e.g. "suddenly")
    trans_dfn    TEXT                           -- extended English explanation
) STRICT;

-- =============================================================================
-- Sentences (gap-fill examples)
-- =============================================================================
CREATE TABLE sentences (
    id         INTEGER PRIMARY KEY,
    card_id    INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    text       TEXT    NOT NULL,                -- full sentence or dialogue (lines joined with \n)
    target     TEXT    NOT NULL,                -- the substring to blank out (validated at insert time)
    created_at TEXT    NOT NULL DEFAULT (datetime('now'))
) STRICT;

-- =============================================================================
-- Sentence inflection hints (only for 동사 / 형용사)
-- =============================================================================
CREATE TABLE sentence_inflection_hints (
    sentence_id  INTEGER PRIMARY KEY REFERENCES sentences(id) ON DELETE CASCADE,
    speech_level TEXT NOT NULL,                 -- hae-che / haeyo-che / hasipsio-che /
                                                --   haera-che / hao-che / hage-che
    tense        TEXT NOT NULL                  -- past / present / future / progressive (동사 only) /
                                                --   retrospective
) STRICT;

-- =============================================================================
-- Sentence translations (English)
-- =============================================================================
CREATE TABLE sentence_translations (
    id          INTEGER PRIMARY KEY,
    sentence_id INTEGER NOT NULL REFERENCES sentences(id) ON DELETE CASCADE,
    translation TEXT    NOT NULL,               -- full English rendering
    UNIQUE(sentence_id)
) STRICT;

-- =============================================================================
-- Card states (per-user SRS)
-- =============================================================================
CREATE TABLE card_states (
    id          INTEGER PRIMARY KEY,
    card_id     INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    stability   REAL    NOT NULL DEFAULT 0,
    difficulty  REAL    NOT NULL DEFAULT 0,
    last_review TEXT,                           -- ISO-8601
    state       TEXT    NOT NULL DEFAULT 'new'
                    CHECK (state IN ('new','learning','review','relearning')),
    UNIQUE(card_id, user_id)
) STRICT;

-- =============================================================================
-- Review history
-- =============================================================================
CREATE TABLE review_history (
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

-- =============================================================================
-- User-card flags (per-user suspension)
-- =============================================================================
CREATE TABLE user_card_flags (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    card_id    INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    suspended  INTEGER NOT NULL DEFAULT 0,      -- 0/1
    flagged_at TEXT    DEFAULT (datetime('now')),
    UNIQUE(user_id, card_id)
) STRICT;

-- =============================================================================
-- User settings (FSRS params, UI prefs)
-- =============================================================================
CREATE TABLE user_settings (
    user_id                  INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    show_percentage          INTEGER NOT NULL DEFAULT 1,
    red_threshold            INTEGER NOT NULL DEFAULT 50,
    yellow_threshold         INTEGER NOT NULL DEFAULT 70,
    theme                    TEXT    NOT NULL DEFAULT 'system',
    desired_retention        REAL    NOT NULL DEFAULT 0.9,
    day_boundary_hour        INTEGER NOT NULL DEFAULT 4,
    auto_progress_on_correct INTEGER NOT NULL DEFAULT 1,
    suppress_new_cards       INTEGER NOT NULL DEFAULT 0,
    auto_progress_delay      INTEGER NOT NULL DEFAULT 0
) STRICT;

-- =============================================================================
-- Invite codes (for signup)
-- =============================================================================
CREATE TABLE invite_codes (
    id              INTEGER PRIMARY KEY,
    code            TEXT    NOT NULL UNIQUE,
    created_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    used_at         TEXT,
    used_by_user_id INTEGER REFERENCES users(id) ON DELETE SET NULL
) STRICT;

-- =============================================================================
-- Indexes for performance
-- =============================================================================

-- Index for custom card metadata by user
CREATE INDEX idx_custom_card_metadata_user ON custom_card_metadata(user_id);

-- Index for card states by user
CREATE INDEX idx_card_states_user ON card_states(user_id, card_id);

-- Index for review history
CREATE INDEX idx_review_history_user_card ON review_history(user_id, card_id, reviewed_at);

-- Index for invite codes
CREATE INDEX idx_invite_codes_code ON invite_codes(code);
CREATE INDEX idx_invite_codes_used ON invite_codes(used_at);