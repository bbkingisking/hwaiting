-- Create invite_codes table
CREATE TABLE IF NOT EXISTS invite_codes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code TEXT NOT NULL UNIQUE,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    used_at DATETIME,
    used_by_user_id INTEGER,
    FOREIGN KEY (used_by_user_id) REFERENCES users(id) ON DELETE SET NULL
);

-- Create index for faster code lookups
CREATE INDEX IF NOT EXISTS idx_invite_codes_code ON invite_codes(code);
CREATE INDEX IF NOT EXISTS idx_invite_codes_used ON invite_codes(used_at);