-- Add auto_progress_on_correct to user_settings table
-- Default is 1 (true), meaning correct answers will auto-progress to the next card
ALTER TABLE user_settings ADD COLUMN auto_progress_on_correct BOOLEAN NOT NULL DEFAULT 1;