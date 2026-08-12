This is a simple CLI interface to a running ../backend. It allows anyone (but is especially useful for AI agents) to go through the database content, one card at a time, like a user would, in a programmatic way, receiving predictable JSON objects for cards and allowing them to POST their answer.

Among other things (see the --help option of the compiled binary for a full list of commands), the `comment` command allows the agent to flag specific things about a card and have that comment be inserted in a separate table in the database that FKs to the card in question.

The `krdict` command is unrelated to the review flow above: it takes a KRDICT target code and dumps the full dictionary entry (via the [`krdict`](https://crates.io/crates/krdict) crate, with English translations included) as JSON, for looking up the words a card is built from. It talks straight to KRDICT, not to the hwaiting backend, and needs `KRDICT_API_KEY` set in the environment.

Going through the content in the same way as a user would (one card at a time) results, I found, in much more useful feedback, compared to just dumping the database and asking the agent to spot what's wrong in it. To that end, `answer` suppresses the card it just graded, right or wrong - the agent isn't here to learn Korean via spaced repetition, so once a card has been seen, it drops out of rotation and the next `review` call surfaces a fresh one. Point the agent at a fresh user made for this purpose (see the `daily_new_card_limit` trick below) so it works through the deck exactly once.

Some useful SQL commands to set up a workflow:

```sql
SELECT COUNT (*) FROM cards; -- gets the total number of cards in the database
```

```sql
UPDATE user_settings SET daily_new_card_limit = 1578 WHERE user_id = 3; -- or however many you want the agent to review at one time
```
