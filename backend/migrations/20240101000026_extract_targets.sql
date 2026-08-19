-- The implicit "target" entity was smeared across three sentence-scoped
-- places: sentences.target (the substring itself), sentence_inflection_hints
-- (tense/speech_level/is_honorific/is_humble -- all properties of that
-- substring, not of the sentence), and sentence_alternative_targets
-- (alternate acceptable forms of that same substring). None of the three is
-- actually about the sentence as prose -- only sentences.text and
-- sentence_translations are -- but all three were named "sentence_*"
-- throughout, which is exactly the vocabulary that let taggers/readers
-- conflate "this sentence's register" with "this target's own conjugation"
-- (see the reverted card_inflection_hints migration for a concrete
-- instance: card 562 was mistagged by reading the sentence's final-ending
-- register instead of the target substring itself).
--
-- Consolidates into one `targets` table. `sentence_id` stays the PK/FK --
-- cardinality is still exactly 1:1, nothing here adds multi-target support,
-- a card's example sentence tests exactly one target the same as before --
-- and renames sentence_alternative_targets -> target_alternatives, re-keyed
-- through `targets` rather than `sentences` directly, since an alternative
-- answer is unambiguously a property of the target, not the sentence.
--
-- Unlike 20240101000025 (schema-only, backfill handled separately), this
-- migration has to carry its own data transfer: `target`, the existing
-- tense/speech_level tags, and existing alternatives are live authoritative
-- content, not optional enrichment -- there's no "backfill later" for data
-- the app would otherwise just lose.
CREATE TABLE targets (
    sentence_id     INTEGER PRIMARY KEY REFERENCES sentences(id) ON DELETE CASCADE,
    form            TEXT NOT NULL,
    speech_level_id INTEGER REFERENCES speech_levels(id),
    tense_id        INTEGER REFERENCES tenses(id),
    is_honorific    INTEGER NOT NULL DEFAULT 0,
    is_humble       INTEGER NOT NULL DEFAULT 0
) STRICT;

INSERT INTO targets (sentence_id, form, speech_level_id, tense_id, is_honorific, is_humble)
SELECT s.id, s.target, sih.speech_level_id, sih.tense_id,
       COALESCE(sih.is_honorific, 0), COALESCE(sih.is_humble, 0)
FROM sentences s
LEFT JOIN sentence_inflection_hints sih ON sih.sentence_id = s.id;

DROP TABLE sentence_inflection_hints;

ALTER TABLE sentences DROP COLUMN target;

CREATE TABLE target_alternatives (
    id          INTEGER PRIMARY KEY,
    sentence_id INTEGER NOT NULL REFERENCES targets(sentence_id) ON DELETE CASCADE,
    alt_target  TEXT NOT NULL,
    UNIQUE(sentence_id, alt_target)
) STRICT;

INSERT INTO target_alternatives (id, sentence_id, alt_target)
SELECT id, sentence_id, alt_target FROM sentence_alternative_targets;

DROP TABLE sentence_alternative_targets;

CREATE INDEX idx_target_alternatives_sentence_id ON target_alternatives(sentence_id);

-- NOTE: the backend (cards/next.rs, cards/check.rs, admin.rs,
-- custom_cards.rs, export_import.rs) and the hwaiting-cards skill's
-- full_card_dump.sql all still reference sentences.target,
-- sentence_inflection_hints, and sentence_alternative_targets as of this
-- migration and will fail once it's applied, until updated to read
-- targets/target_alternatives instead.
