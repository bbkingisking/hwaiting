ALTER TABLE passkeys ADD COLUMN label TEXT
    CHECK (label IS NULL OR length(label) <= 100);
