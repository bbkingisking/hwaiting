-- Free-form content-review notes on cards (e.g. "tense looks wrong here"),
-- left by whoever's doing content review - not shown in the review UI,
-- purely a backlog for admin triage. See POST /api/cards/{id}/comment.
CREATE TABLE IF NOT EXISTS card_comments (
    id         INTEGER PRIMARY KEY,
    card_id    INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body       TEXT    NOT NULL,
    created_at TEXT    NOT NULL DEFAULT (datetime('now'))
) STRICT;

CREATE INDEX IF NOT EXISTS idx_card_comments_card_id ON card_comments(card_id);
CREATE INDEX IF NOT EXISTS idx_card_comments_user_id ON card_comments(user_id);
