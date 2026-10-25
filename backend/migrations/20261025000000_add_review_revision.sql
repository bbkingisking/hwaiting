-- Per-user/card concurrency marker. Missing state rows are revision zero too.
ALTER TABLE cards_states
ADD COLUMN review_revision INTEGER NOT NULL DEFAULT 0
CHECK (review_revision >= 0);
