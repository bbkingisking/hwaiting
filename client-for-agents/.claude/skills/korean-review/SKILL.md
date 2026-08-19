---
name: hwaiting-card-review
description: Review hwaiting flashcard content for errors — missing alternative answers, wrong grammar pattern, tense, politeness level, or English translation — and log comments for follow-up.
---

# Reviewing hwaiting cards

You are reviewing the content of a Korean-learning flashcard app, not learning from it. Your job is to work through cards, answer them, and notice when a card itself is wrong — a missing alternative, a mislabeled tense, a bad translation — and leave a comment when it is.

## Interface: hwaiting-agent

Your only interface to the app is a single static binary, `hwaiting-agent`. Run it with --help to famliarize yourself with the commands that are available to you.

## Workflow

1. `login`, then `lookups`, once each per session.
2. `review` to get a card. If `card` is `null`, stop.
3. Form your answer from the hints given, `answer <card_id> <answer>`.
4. Compare what `answer` revealed against what `review` showed you (see "Common errors" below). If something looks off, `comment`.
5. If you are not sure, use the `krdict` command to get more information about the card from the official KRDict API. Use this liberally to verify assumptions.
6. Go back to 2.

## Common errors to watch for

- **Missing alternatives** — by far the most common. If `review`'s hints support your answer but `answer` comes back `correct: false`, it's very likely a contracted form, a 1:1 synonym, or another variant that should be in `alternatives` but isn't. The test: given all the hints for this specific card, would a native speaker plausibly type the alternative form? If so, it belongs. Example: 조금/좀 are interchangeable in a lot of contexts, so they're alternatives of each other. Only reach for this once you've actually gotten a surprising `correct: false` — don't pre-emptively brainstorm every possible alternative for cards that already accepted the obvious one.
- **Wrong or missing `grammar_pattern`** — see below, this one needs judgment.
- **Mismatched `tense`**
- **Mismatched `speech_level`**
- **Wrong English translation**

### Grammar patterns need judgment, not just pattern-matching

Not every grammar ending deserves its own `grammar_pattern` entry. `-서`, for instance, is common enough and discernible enough from context/translation alone that tagging it would just add noise — a learner should lean on context for endings like that. The ones worth tagging are the ones that are genuinely hard to infer from the English translation alone — e.g. `backdrop` (-는데/-(으)ㄴ데), where the translation might lead a learner to type the plain form instead of the backdrop one.

So when you see a card that seems to be using a pattern from `lookups`'s `grammar_pattern` list but has no `grammar_pattern` set (or the wrong one), ask whether it's actually the kind of pattern a learner would miss from context — not just whether the ending technically matches. The philosophy above (why `-서` doesn't get tagged but `backdrop` does) is the part `lookups` can't tell you; use it alongside whatever's currently in that table, not in place of it.
