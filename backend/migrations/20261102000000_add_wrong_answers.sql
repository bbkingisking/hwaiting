CREATE TABLE wrong_answers (
    review_id          INTEGER PRIMARY KEY REFERENCES review_history(id),
    target_sentence_id INTEGER NOT NULL REFERENCES targets(sentence_id),
    answer             TEXT    NOT NULL
) STRICT;

CREATE INDEX idx_wrong_answers_target_sentence_id
    ON wrong_answers(target_sentence_id);
