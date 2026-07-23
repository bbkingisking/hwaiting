-- Typical grammatical endings for each pattern, revealed in the UI after the card is answered.
ALTER TABLE grammar_patterns ADD COLUMN endings TEXT NOT NULL DEFAULT '';

UPDATE grammar_patterns SET endings = '-네(요), -군(요)' WHERE slug = 'realization';
UPDATE grammar_patterns SET endings = '-더니' WHERE slug = 'witnessed-then';
UPDATE grammar_patterns SET endings = '-는데, -(으)ㄴ데' WHERE slug = 'backdrop';
UPDATE grammar_patterns SET endings = '-느라고' WHERE slug = 'mishap';
UPDATE grammar_patterns SET endings = '-길래' WHERE slug = 'prompted';
UPDATE grammar_patterns SET endings = '-거든(요)' WHERE slug = 'shared-context';
UPDATE grammar_patterns SET endings = '-다면서(요)' WHERE slug = 'confirm-surprise';
UPDATE grammar_patterns SET endings = '-다니(요)' WHERE slug = 'disbelief';
UPDATE grammar_patterns SET endings = '-대(요), -래(요)' WHERE slug = 'reported';
UPDATE grammar_patterns SET endings = '-(으)ㄹ 뻔하다' WHERE slug = 'near-miss';
UPDATE grammar_patterns SET endings = '-아/어 버리다' WHERE slug = 'regret-completion';
UPDATE grammar_patterns SET endings = '-기로 하다' WHERE slug = 'decision';
UPDATE grammar_patterns SET endings = '-(으)려고(요)' WHERE slug = 'intent-trailing';
UPDATE grammar_patterns SET endings = '-(으)러' WHERE slug = 'motion-purpose';
UPDATE grammar_patterns SET endings = '-도록' WHERE slug = 'extent';
UPDATE grammar_patterns SET endings = '-(으)ㄹ수록' WHERE slug = 'escalation';
