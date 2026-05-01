-- Add suppress_new_cards to user_settings table
ALTER TABLE user_settings ADD COLUMN suppress_new_cards BOOLEAN NOT NULL DEFAULT 0;
