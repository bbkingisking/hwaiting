-- A review still advances one card's FSRS state, but records which of its
-- sentence prompts was actually shown. Older histories are only attributable
-- where the card has exactly one sentence; ambiguous rows remain NULL.
ALTER TABLE review_history ADD COLUMN sentence_id INTEGER REFERENCES sentences(id);

UPDATE review_history
SET sentence_id = (
    SELECT wa.target_sentence_id FROM wrong_answers wa WHERE wa.review_id = review_history.id
)
WHERE EXISTS (
    SELECT 1 FROM wrong_answers wa
    JOIN sentences s ON s.id = wa.target_sentence_id
    WHERE wa.review_id = review_history.id AND s.card_id = review_history.card_id
);

UPDATE review_history
SET sentence_id = (SELECT s.id FROM sentences s WHERE s.card_id = review_history.card_id)
WHERE sentence_id IS NULL
  AND (SELECT COUNT(*) FROM sentences s WHERE s.card_id = review_history.card_id) = 1;

CREATE INDEX idx_review_history_user_sentence_id
    ON review_history(user_id, sentence_id, id);
