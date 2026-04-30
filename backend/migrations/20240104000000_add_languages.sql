-- Create languages table
CREATE TABLE IF NOT EXISTS languages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL UNIQUE
);

-- Insert Korean as the first language
INSERT INTO languages (code, name) VALUES ('ko', 'Korean');

-- Add language_id to words table
ALTER TABLE words ADD COLUMN language_id INTEGER REFERENCES languages(id);

-- Set all existing words to Korean
UPDATE words SET language_id = (SELECT id FROM languages WHERE code = 'ko');

-- Make language_id NOT NULL after setting values
-- (SQLite doesn't support ALTER COLUMN, so we'll enforce this in the app)

-- Add target_language_id to users table (nullable - null means not chosen yet)
ALTER TABLE users ADD COLUMN target_language_id INTEGER REFERENCES languages(id);

-- Set seok's target language to Korean
UPDATE users SET target_language_id = (SELECT id FROM languages WHERE code = 'ko') WHERE username = 'seok';

-- Create indexes
CREATE INDEX IF NOT EXISTS idx_words_language ON words(language_id);
CREATE INDEX IF NOT EXISTS idx_users_target_language ON users(target_language_id);