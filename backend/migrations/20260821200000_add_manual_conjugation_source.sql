-- Some words (갖다, 뵙다, 묻다, ...) have conjugation patterns too
-- idiosyncratic for the generator script to safely handle -- rather than
-- carve a special-case branch into the generator for a handful of words,
-- it stays away from them entirely (see the TRICKY_WORDS blacklist in
-- scripts/gen_conjugation_matrix/src/bin/gen_yongcat.rs) and their
-- correct forms are hand-verified and inserted directly instead, tagged
-- with this source. See backend/seed/manual_conjugation_overrides.sql.
INSERT OR IGNORE INTO conjugation_matrix_sources (id, slug) VALUES (3, 'manual');
