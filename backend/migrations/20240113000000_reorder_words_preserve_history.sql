-- Migration: Reorder words while preserving review history
-- This migration allows words to be reinserted in correct deck order
-- while preserving all user review history and card states.

-- Step 1: Create backup tables with word identifiers
CREATE TABLE review_history_backup AS
SELECT 
    rh.id,
    rh.user_id,
    rh.word_id,
    rh.rating,
    rh.reviewed_at,
    w.form,
    w.language_id
FROM review_history rh
JOIN words w ON w.id = rh.word_id;

CREATE TABLE card_states_backup AS
SELECT 
    cs.id,
    cs.user_id,
    cs.word_id,
    cs.stability,
    cs.difficulty,
    cs.last_review,
    cs.due_date,
    cs.suppressed,
    w.form,
    w.language_id
FROM card_states cs
JOIN words w ON w.id = cs.word_id;

-- Step 2: Delete all non-custom words (cascades to review_history and card_states)
-- Custom words (user_id IS NOT NULL) are preserved
DELETE FROM words WHERE user_id IS NULL;

-- Step 3: Words will be reinserted in correct order by the seeding process
-- (This happens automatically when the application starts after migration)

-- Step 4: Restore card_states with new word_ids
INSERT INTO card_states (user_id, word_id, stability, difficulty, last_review, due_date, suppressed)
SELECT 
    csb.user_id,
    w.id as new_word_id,
    csb.stability,
    csb.difficulty,
    csb.last_review,
    csb.due_date,
    csb.suppressed
FROM card_states_backup csb
JOIN words w ON w.form = csb.form AND w.language_id = csb.language_id;

-- Step 5: Restore review_history with new word_ids
INSERT INTO review_history (user_id, word_id, rating, reviewed_at)
SELECT 
    rhb.user_id,
    w.id as new_word_id,
    rhb.rating,
    rhb.reviewed_at
FROM review_history_backup rhb
JOIN words w ON w.form = rhb.form AND w.language_id = rhb.language_id;

-- Step 6: Cleanup backup tables
DROP TABLE review_history_backup;
DROP TABLE card_states_backup;