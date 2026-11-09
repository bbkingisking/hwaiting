-- NULL means a new account has not chosen its study timezone yet.
ALTER TABLE users_settings ADD COLUMN study_timezone TEXT;
-- Include existing accounts whose settings were never materialized.
INSERT INTO users_settings (user_id) SELECT id FROM users WHERE true
ON CONFLICT(user_id) DO NOTHING;
-- Startup resolves this marker once, using the deployment's current timezone.
UPDATE users_settings SET study_timezone = '';
