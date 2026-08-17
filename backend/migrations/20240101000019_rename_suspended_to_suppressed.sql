-- Align the DB column with the API terminology: mutation endpoints and
-- response schemas call this "suppress"/"suppressed" (see /api/cards/{id}/suppress,
-- /api/cards/{id}/unsuppress, /api/cards/suppressed), but the column and its
-- index were still named after the older "suspended" wording.
ALTER TABLE user_card_flags RENAME COLUMN suspended TO suppressed;

DROP INDEX IF EXISTS idx_user_card_flags_suspended;
CREATE INDEX IF NOT EXISTS idx_user_card_flags_suppressed ON user_card_flags(suppressed);
