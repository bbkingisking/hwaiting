-- sentence_inflection_hints was keyed by sentence_id, which invites reading
-- speech_level/tense as describing the sentence's overall register (e.g.
-- its sentence-final ending) rather than what it's actually meant to be:
-- the tense/formality of the card's target conjugated form. Every card
-- currently has exactly one sentence, so re-key straight to card_id and
-- rename the table so the intent is visible at the schema level.
--
-- This is a pure re-key, not a re-tag: whatever speech_level/tense a row
-- already had is carried over unchanged. Some of that data was tagged by
-- eyeballing the sentence's final ending rather than the target form
-- itself, so it may now read as wrong for its card (e.g. card 562) -- that's
-- a data-quality cleanup to do separately, not something this migration
-- attempts to fix.
CREATE TABLE card_inflection_hints (
    card_id         INTEGER PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
    speech_level_id INTEGER REFERENCES speech_levels(id),
    tense_id        INTEGER REFERENCES tenses(id)
) STRICT;

INSERT INTO card_inflection_hints (card_id, speech_level_id, tense_id)
SELECT s.card_id, sih.speech_level_id, sih.tense_id
FROM sentence_inflection_hints sih
JOIN sentences s ON s.id = sih.sentence_id;

DROP TABLE sentence_inflection_hints;
