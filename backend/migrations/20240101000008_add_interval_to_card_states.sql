-- Add interval_days column to card_states for correct scheduling.
-- Previously, the due-date query used 'stability' (an FSRS memory parameter)
-- instead of the scheduling interval. This column stores the actual interval
-- in days returned by fsrs.next_states().
ALTER TABLE card_states ADD COLUMN interval_days REAL NOT NULL DEFAULT 0;

-- Backfill: for each card that has been reviewed, recompute the interval from
-- the last review_history entry's scheduled_days.  We cannot re-derive the
-- exact FSRS interval because the old scheduled_days also stored stability,
-- but we can at least set interval_days = stability so the card is at least
-- as spaced out as it was before (better than 0).  A proper recompute would
-- require running FSRS in Rust; instead we just set interval to
-- scheduled_days from the last review, which is the closest available value.
UPDATE card_states
SET interval_days = (
    SELECT rh.scheduled_days
    FROM review_history rh
    WHERE rh.card_id = card_states.card_id
      AND rh.user_id = card_states.user_id
    ORDER BY rh.reviewed_at DESC
    LIMIT 1
)
WHERE last_review IS NOT NULL;
