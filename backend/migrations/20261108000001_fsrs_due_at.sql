-- Stability is the interval at 90% retention, not the scheduled interval at
-- arbitrary retention. Backfilled by cards::scheduling::backfill_due_dates
-- after SQL migrations, using the fsrs crate and each user's trained model.
ALTER TABLE cards_states ADD COLUMN due_at INTEGER;
CREATE INDEX idx_cards_states_due ON cards_states(user_id, due_at);
