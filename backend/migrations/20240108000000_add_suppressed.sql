-- Add suppressed column to card_states table
ALTER TABLE card_states ADD COLUMN suppressed BOOLEAN NOT NULL DEFAULT 0;

-- Create index for performance when filtering suppressed cards
CREATE INDEX IF NOT EXISTS idx_card_states_suppressed ON card_states(user_id, suppressed);