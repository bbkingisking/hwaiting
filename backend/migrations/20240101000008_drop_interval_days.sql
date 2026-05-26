-- Remove the interval_days column that was added by mistake.
-- FSRS interval equals stability when desired_retention = 0.9, so the
-- separate column was unnecessary.

-- Drop the orphaned migration record so sqlx doesn't complain about
-- the missing 20240101000008_add_interval_to_card_states.sql file.
DELETE FROM _sqlx_migrations WHERE version = 20240101000008;

-- SQLite 3.35+ supports DROP COLUMN.
ALTER TABLE card_states DROP COLUMN interval_days;
