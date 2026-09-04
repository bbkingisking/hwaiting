-- The custom-cards feature (backend/src/custom_cards.rs, the "Custom Cards"
-- dialog, and everything else that read/wrote custom_card_metadata) has been
-- removed. This drops its table and the `cards` rows it was the only thing
-- pointing at -- the ones a user typed in themselves rather than KRDICT- or
-- admin-sourced cards.
--
-- No FK cascade to lean on for any of this: SqliteConnectOptions disables
-- `foreign_keys` app-wide (see db.rs's connection options), so the
-- ON DELETE CASCADE these tables declare has never actually been enforced --
-- deleting a `cards` row alone leaves its cards_translations/sentences/etc.
-- rows behind as orphans. custom_cards::delete_custom_card already ran into
-- this without knowing it: it only ever deleted the custom_card_metadata and
-- cards rows for the card being deleted, trusting a CASCADE that silently
-- never ran, so every custom card ever deleted through the app before this
-- migration left its sentences/translations/targets/etc. behind pointing at
-- a card_id that no longer exists.
--
-- `cards` is the only table this codebase has ever deleted rows from (here,
-- and previously custom_cards.rs and export_import.rs's overwrite-import
-- path -- both gone/rewritten as of this change), so the orphan sweep below
-- is safe: anything it finds can only be leftover custom-card debris, never
-- collateral damage to real data.

DELETE FROM cards WHERE id IN (SELECT card_id FROM custom_card_metadata);

DROP TABLE custom_card_metadata;

DELETE FROM targets_alternatives WHERE sentence_id IN (
    SELECT id FROM sentences WHERE card_id NOT IN (SELECT id FROM cards)
);
DELETE FROM sentences_translations WHERE sentence_id IN (
    SELECT id FROM sentences WHERE card_id NOT IN (SELECT id FROM cards)
);
DELETE FROM targets WHERE sentence_id IN (
    SELECT id FROM sentences WHERE card_id NOT IN (SELECT id FROM cards)
);
DELETE FROM sentences WHERE card_id NOT IN (SELECT id FROM cards);
DELETE FROM cards_translations WHERE card_id NOT IN (SELECT id FROM cards);
DELETE FROM cards_comments WHERE card_id NOT IN (SELECT id FROM cards);
DELETE FROM cards_states WHERE card_id NOT IN (SELECT id FROM cards);
DELETE FROM review_history WHERE card_id NOT IN (SELECT id FROM cards);
DELETE FROM users_card_flags WHERE card_id NOT IN (SELECT id FROM cards);
DELETE FROM conjugation_matrix_cards WHERE card_id NOT IN (SELECT id FROM cards);
