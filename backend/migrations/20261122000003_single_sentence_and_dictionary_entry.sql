-- A card is one review item: one Korean sentence, with any number of translations.
-- The existing deferred required_sentence_id foreign key enforces at least one;
-- this unique index supplies the at-most-one half of that invariant.
CREATE UNIQUE INDEX sentences_one_per_card ON sentences(card_id);

-- KRDICT identifies dictionary entries, including distinct homonyms. SQLite
-- permits multiple NULLs here, so manually authored cards remain supported.
-- Do not make cards.word unique: different entries may share their spelling.
CREATE UNIQUE INDEX cards_unique_krdict_id ON cards(krdict_id) WHERE krdict_id IS NOT NULL;
