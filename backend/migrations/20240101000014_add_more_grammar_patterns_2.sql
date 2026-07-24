-- Seed additional grammar patterns (not yet associated with any cards).
INSERT INTO grammar_patterns (slug, label, tooltip, endings) VALUES
    ('for-someone', 'For-someone', 'Marks the action as done for or toward someone else''s benefit, even when the English translation doesn''t make that explicit.', '-아/어 주다'),
    ('gentle-question', 'Gentle question', 'A softer, more considerate question ending than -아/어요, common when asking about someone''s feelings or situation.', '-(으)ㄴ가요, -나요');
