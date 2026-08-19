-- Reverts 20240101000023_link_inflection_hints_to_cards.sql.
--
-- That migration re-keyed inflection hints from sentence_id to card_id on
-- the theory that speech_level/tense are card-level facts. They're not:
-- `target` (the conjugated form they describe) lives on the *sentence* row,
-- not the card row, so a hint has to reference whichever sentence holds the
-- target it's tagging. Keying to card_id only worked by coincidence, since
-- every card currently has exactly one sentence -- it expressed no real FK
-- relationship to `target` at all, and would break the moment a card got a
-- second example sentence.
--
-- The actual problem this was trying to fix (card 562 read as mistagged)
-- was a data/labeling issue -- the hint was inferred from the sentence's
-- final-ending register instead of the target substring's own conjugation
-- -- not a wrong-parent-table issue. That gets fixed by scoping
-- speech_level/tense to `target` explicitly wherever they're read/edited,
-- not by moving the FK.
CREATE TABLE sentence_inflection_hints (
    sentence_id     INTEGER PRIMARY KEY REFERENCES sentences(id) ON DELETE CASCADE,
    speech_level_id INTEGER REFERENCES speech_levels(id),
    tense_id        INTEGER REFERENCES tenses(id)
) STRICT;

INSERT INTO sentence_inflection_hints (sentence_id, speech_level_id, tense_id)
SELECT s.id, cih.speech_level_id, cih.tense_id
FROM card_inflection_hints cih
JOIN sentences s ON s.card_id = cih.card_id;

DROP TABLE card_inflection_hints;

CREATE INDEX IF NOT EXISTS idx_sentence_inflection_hints_sentence_id ON sentence_inflection_hints(sentence_id);
