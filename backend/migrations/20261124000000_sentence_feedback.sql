-- Card comments have no existing data to preserve.
DROP TABLE cards_comments;

CREATE TABLE feedback (
    id INTEGER PRIMARY KEY,
    sentence_id INTEGER NOT NULL REFERENCES sentences(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;

CREATE INDEX idx_feedback_sentence_id ON feedback(sentence_id);
CREATE INDEX idx_feedback_user_id ON feedback(user_id);
