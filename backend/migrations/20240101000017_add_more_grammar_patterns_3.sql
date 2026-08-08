-- Seed additional grammar patterns (not yet associated with any cards).
INSERT INTO grammar_patterns (slug, label, tooltip, endings) VALUES
    ('indirect-question', 'Indirect question', 'Turns a question into a noun-like clause embedded in a larger sentence — needs a following verb like 모르다, 알다, or 궁금하다.', '-는지, -(으)ㄴ지');
