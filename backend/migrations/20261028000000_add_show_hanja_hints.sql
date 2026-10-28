ALTER TABLE users_settings
ADD COLUMN show_hanja_hints INTEGER NOT NULL DEFAULT 1
CHECK (show_hanja_hints IN (0, 1));
