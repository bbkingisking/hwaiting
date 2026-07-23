-- Seed additional grammar patterns (not yet associated with any cards).
INSERT INTO grammar_patterns (slug, label, tooltip) VALUES
    ('witnessed-then', 'Witnessed-then', 'Reports something the speaker personally observed happening, followed by a related or contrasting result.'),
    ('backdrop', 'Backdrop', 'Sets up context or a soft contrast rather than a hard "but" — often left dangling to imply something unsaid.'),
    ('mishap', 'Mishap', 'Cites a cause that led to something negative or wasted, not neutral causation.'),
    ('prompted', 'Prompted', 'Cites something the speaker noticed that triggered their own next action; can''t be used for someone else''s reaction.'),
    ('shared-context', 'Shared-context', 'Gives a reason assuming the listener didn''t know it yet or will find it relevant.'),
    ('confirm-surprise', 'Confirm-surprise', 'Checks something the speaker heard against the listener, often with a note of surprise.'),
    ('disbelief', 'Disbelief', 'Reacts to reported information with shock or disbelief, more emotional than a plain quote.'),
    ('reported', 'Reported', 'Compressed hearsay — "I heard X" — collapsed from the fuller quotative form.'),
    ('near-miss', 'Near-miss', 'Something almost happened but didn''t; easy to mistranslate as if it did happen.'),
    ('regret-completion', 'Regret/completion', 'Marks an action as fully finished, often with a tinge of regret or relief.'),
    ('decision', 'Decision', 'Marks a decision being made or already settled, not just an intention in progress.'),
    ('intent-trailing', 'Intent (trailing)', 'States a goal or plan as the reason for something, often left unfinished at the end of the sentence.'),
    ('motion-purpose', 'Motion-purpose', '"In order to," restricted to verbs of literal movement like 가다 and 오다.'),
    ('extent', 'Extent', '"So that" or "to the point of," implying more effort or a firmer limit than a plain purpose ending.'),
    ('escalation', 'Escalation', 'The more..., the more... — pairs with itself across two clauses.');
