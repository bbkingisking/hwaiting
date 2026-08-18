-- Seed additional grammar patterns (not yet associated with any cards).
INSERT INTO grammar_patterns (slug, label, tooltip, endings) VALUES
    ('connective-seo', 'Sequential/cause', 'Links two clauses where the first happens before and enables the second — sequence when both are actions, cause when the second is a state or involuntary result.', '-아/어서'),
    ('adnominal', 'Adnominal', 'Modifies a following noun; the specific form shifts with tense — past/state, present ongoing, or future/hypothetical — rather than being one fixed shape.', '-(으)ㄴ, -는, -(으)ㄹ'),
    ('adverbializer', 'Adverbializer', 'Turns an adjective into an adverb or sets a standard/result for the clause that follows — "so as to," "in a way that," often paired with verbs like 하다, 되다, 만들다.', '-게'),
    ('causal-nikka', 'Cause/reason', 'States a reason the speaker has just realized or is asserting as justification, and — unlike -아/어서 — can lead into a command, suggestion, or future-tense clause.', '-(으)니까'),
    ('contrast-jiman', 'Contrast', 'A plain, general-purpose "but" connecting two clauses, with no cause-effect or backdrop nuance the way -는데/-(으)ㄴ데 carries.', '-지만'),
    ('connective-go', 'Sequential list', 'Strings together actions or states as a simple list or plain sequence, with no cause-effect relationship implied between them.', '-고'),
    ('conditional-myeon', 'Conditional', '"If" or "when" — sets up a hypothetical, habitual, or general condition for the clause that follows.', '-(으)면');
