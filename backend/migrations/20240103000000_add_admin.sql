-- Add is_admin column to users table
ALTER TABLE users ADD COLUMN is_admin BOOLEAN NOT NULL DEFAULT 0;

-- Create index for faster admin lookups
CREATE INDEX IF NOT EXISTS idx_users_admin ON users(is_admin);