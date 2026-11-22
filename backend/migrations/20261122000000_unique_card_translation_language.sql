-- Each card has one word/definition translation per language, matching the
-- sentence translation model. Existing duplicates must be resolved explicitly;
-- do not discard potentially distinct translations during migration.
CREATE UNIQUE INDEX idx_cards_translations_card_language
    ON cards_translations(card_id, language_id);
