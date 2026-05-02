-- Add auto_progress_delay to user_settings table
-- Default is 0 (milliseconds), meaning no delay before auto-progressing
ALTER TABLE user_settings ADD COLUMN auto_progress_delay INTEGER NOT NULL DEFAULT 0;
