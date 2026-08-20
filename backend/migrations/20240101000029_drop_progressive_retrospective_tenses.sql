-- 'progressive' and 'retrospective' predate the grammar_patterns system and
-- don't actually belong in `tenses`: tense_id is single-valued per target,
-- but progressive aspect (-고 있다) and retrospective evidentiality (-더-)
-- are orthogonal to tense, not alternatives to it -- a target can be e.g.
-- past-tense *and* progressive at once ("고민하고 있었어요"), and forcing
-- that into one tense_id column meant the past marking got silently
-- dropped in favor of "progressive" (see the new `progressive` grammar
-- pattern from migration 20240101000028, which is the correct home for
-- this instead).
--
-- Targets currently tagged with these two tense values have their tense_id
-- cleared rather than guessed at here -- each one needs its real tense
-- (present/past/future) and, where applicable, the corresponding grammar
-- pattern (`progressive`, `witnessed-then`, etc.) re-derived by hand from
-- its sentence, not inferred mechanically by this migration.
UPDATE targets SET tense_id = NULL
WHERE tense_id IN (SELECT id FROM tenses WHERE slug IN ('progressive', 'retrospective'));

DELETE FROM tenses WHERE slug IN ('progressive', 'retrospective');
