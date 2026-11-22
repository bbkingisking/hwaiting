-- NULL means onboarding has not selected a content language yet.
-- Interface language is independent. Existing accounts are not silently
-- assigned a language; the client must still send its explicit selection.
ALTER TABLE users_settings ADD COLUMN translation_language TEXT
    REFERENCES languages(slug)
    CHECK (translation_language GLOB '[a-z][a-z][a-z]');
