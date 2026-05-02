-- Make stability and difficulty nullable to support suppressed new cards
-- NULL means the card has no FSRS state yet (never been reviewed)

-- SQLite doesn't support ALTER COLUMN directly, so we need to recreate the table
CREATE TABLE card_states_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    word_id INTEGER NOT NULL,
    -- FSRS MemoryState (NULL for cards that have never been reviewed)
    stability REAL,
    difficulty REAL,
    -- Scheduling
    last_review DATETIME,
    due_date DATETIME,
    suppressed BOOLEAN NOT NULL DEFAULT 0,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (word_id) REFERENCES words(id) ON DELETE CASCADE,
    UNIQUE(user_id, word_id)
);

-- Copy existing data
INSERT INTO card_states_new (id, user_id, word_id, stability, difficulty, last_review, due_date, suppressed)
SELECT id, user_id, word_id, stability, difficulty, last_review, due_date, suppressed
FROM card_states;

-- Drop old table and rename new one
DROP TABLE card_states;
ALTER TABLE card_states_new RENAME TO card_states;

-- Recreate indexes
CREATE INDEX IF NOT EXISTS idx_card_states_user_due ON card_states(user_id, due_date);
CREATE INDEX IF NOT EXISTS idx_card_states_suppressed ON card_states(user_id, suppressed);