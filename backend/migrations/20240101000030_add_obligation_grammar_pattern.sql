-- Seed additional grammar patterns (not yet associated with any cards).
INSERT INTO grammar_patterns (slug, label, tooltip, endings) VALUES
    ('obligation', 'Obligation/necessity', 'Marks the clause as a necessary condition, typically paired with 하다/되다 to mean "must" — easy to confuse with a plain intent or conditional ending.', '-아/어야 (하다/되다)');
