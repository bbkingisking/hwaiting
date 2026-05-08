ALTER TABLE user_settings ADD COLUMN history_colorized_area    INTEGER NOT NULL DEFAULT 0;
ALTER TABLE user_settings ADD COLUMN history_colored_dots      INTEGER NOT NULL DEFAULT 0;
ALTER TABLE user_settings ADD COLUMN history_threshold_lines   INTEGER NOT NULL DEFAULT 0;