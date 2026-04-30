-- Add user_id column to words table to support custom cards
-- NULL = official/shared card, NOT NULL = custom user card
ALTER TABLE words ADD COLUMN user_id INTEGER REFERENCES users(id) ON DELETE CASCADE;

-- Add created_at column for tracking when cards are created
-- SQLite doesn't support DEFAULT CURRENT_TIMESTAMP on ALTER TABLE, so we add it nullable first
ALTER TABLE words ADD COLUMN created_at DATETIME;

-- Update existing rows to have a created_at value
UPDATE words SET created_at = CURRENT_TIMESTAMP WHERE created_at IS NULL;

-- Create index for efficient querying of custom cards
CREATE INDEX IF NOT EXISTS idx_words_user_language ON words(user_id, language_id);

-- Create index for querying official cards (user_id IS NULL)
CREATE INDEX IF NOT EXISTS idx_words_official ON words(language_id) WHERE user_id IS NULL;