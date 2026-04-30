-- Add day_boundary_hour to user_settings table
-- Default is 4 (4:00 AM), meaning reviews before 4 AM count as the previous day
ALTER TABLE user_settings ADD COLUMN day_boundary_hour INTEGER NOT NULL DEFAULT 4;