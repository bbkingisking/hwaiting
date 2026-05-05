-- User settings table
CREATE TABLE IF NOT EXISTS user_settings (
    user_id                  INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    show_percentage          INTEGER NOT NULL DEFAULT 1,
    red_threshold            INTEGER NOT NULL DEFAULT 80,
    yellow_threshold         INTEGER NOT NULL DEFAULT 90,
    day_boundary_hour        INTEGER NOT NULL DEFAULT 4,
    auto_progress_on_correct INTEGER NOT NULL DEFAULT 0,
    auto_progress_delay      INTEGER NOT NULL DEFAULT 1500,
    desired_retention        REAL    NOT NULL DEFAULT 0.9,
    daily_new_card_limit     INTEGER NOT NULL DEFAULT 20
) STRICT;

-- Index
CREATE INDEX IF NOT EXISTS idx_user_settings_user_id ON user_settings(user_id);