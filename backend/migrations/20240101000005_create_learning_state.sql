-- Card states table (FSRS learning state)
CREATE TABLE IF NOT EXISTS card_states (
    user_id INTEGER NOT NULL,
    card_id INTEGER NOT NULL,
    stability REAL NOT NULL DEFAULT 0.0,
    difficulty REAL NOT NULL DEFAULT 0.0,
    last_review TEXT,
    state TEXT NOT NULL DEFAULT 'new',
    PRIMARY KEY (user_id, card_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (card_id) REFERENCES cards(id) ON DELETE CASCADE
);

-- Review history table (tracks all reviews)
CREATE TABLE IF NOT EXISTS review_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    card_id INTEGER NOT NULL,
    rating TEXT NOT NULL,
    scheduled_days REAL,
    elapsed_days REAL,
    reviewed_at TEXT NOT NULL DEFAULT (datetime('now')),
    stability REAL,
    difficulty REAL,
    state TEXT,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (card_id) REFERENCES cards(id) ON DELETE CASCADE
);

-- User card flags table (suspended/flagged cards)
CREATE TABLE IF NOT EXISTS user_card_flags (
    user_id INTEGER NOT NULL,
    card_id INTEGER NOT NULL,
    suspended INTEGER NOT NULL DEFAULT 0,
    flagged_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, card_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (card_id) REFERENCES cards(id) ON DELETE CASCADE
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_card_states_user_id ON card_states(user_id);
CREATE INDEX IF NOT EXISTS idx_card_states_card_id ON card_states(card_id);
CREATE INDEX IF NOT EXISTS idx_card_states_last_review ON card_states(last_review);
CREATE INDEX IF NOT EXISTS idx_review_history_user_id ON review_history(user_id);
CREATE INDEX IF NOT EXISTS idx_review_history_card_id ON review_history(card_id);
CREATE INDEX IF NOT EXISTS idx_review_history_reviewed_at ON review_history(reviewed_at);
CREATE INDEX IF NOT EXISTS idx_user_card_flags_user_id ON user_card_flags(user_id);
CREATE INDEX IF NOT EXISTS idx_user_card_flags_card_id ON user_card_flags(card_id);
CREATE INDEX IF NOT EXISTS idx_user_card_flags_suspended ON user_card_flags(suspended);