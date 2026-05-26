CREATE TABLE IF NOT EXISTS user_fsrs_parameters (
    user_id    INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    parameters TEXT NOT NULL
) STRICT;
