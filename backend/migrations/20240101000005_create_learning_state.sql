-- Card states table (FSRS learning state)
CREATE TABLE IF NOT EXISTS card_states (
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

-- Review history table (tracks all reviews)
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

-- User card flags table (suspended/flagged cards)
CREATE TABLE IF NOT EXISTS user_card_flags (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    card_id    INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    suspended  INTEGER NOT NULL DEFAULT 0,
    flagged_at TEXT    DEFAULT (datetime('now')),
    UNIQUE(user_id, card_id)
) STRICT;

-- Indexes
CREATE INDEX IF NOT EXISTS idx_card_states_user ON card_states(user_id, card_id);
CREATE INDEX IF NOT EXISTS idx_review_history_user_card ON review_history(user_id, card_id, reviewed_at);
CREATE INDEX IF NOT EXISTS idx_user_card_flags_user_id ON user_card_flags(user_id);
CREATE INDEX IF NOT EXISTS idx_user_card_flags_card_id ON user_card_flags(card_id);
CREATE INDEX IF NOT EXISTS idx_user_card_flags_suspended ON user_card_flags(suspended);