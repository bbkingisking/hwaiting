# hwaiting

Korean spaced-repetition flashcards. An axum + SQLite backend (`backend/`) serves a React/Vite SPA (`frontend/`) from a single binary; reviews are scheduled with FSRS and answered by typing the missing word into a Korean sentence.

Build the frontend with `npm run build` in `frontend/` — that runs `tsc` first, so it is also the typecheck. There is no test suite.

## Driving this app in a browser

The UI is deliberately shaped so an agent can work from the accessibility tree instead of from screenshots and pixel coordinates. What follows is the contract that makes that possible; it is worth reading before automating anything, because two parts of the review loop are counter-intuitive.

A plain text read of the page captures nearly the whole card in one call — the badges, the sentence with its blank, both translations, and the grading verdict as prose rather than as colour. Use it as the primary read, and the accessibility tree for the things you intend to click or type into. The blank's pixel position shifts substantially between cards, so target the "Answer" textbox by reference and never by coordinate.

### Landmarks

`main` is the review area and the only place cards appear. `header` holds the user menu (absent when logged out). `footer` is the stats bar, named "Review stats". The document is `lang="en"`; Korean runs are marked `lang="ko"`.

### Waiting for a card to advance

This is the part that will bite you. Advancing shows **no loading state on purpose** — the outgoing card stays on screen so there is no flash — and the `Flashcard` subtree is **not remounted**, because the answer input must stay mounted to keep the mobile keyboard open across cards. So the naive signals do not work.

Use these instead:

- `main[aria-busy]` is `true` from the moment a review is submitted until the next card is rendered.
- `article[data-card-id]` identifies the current card and is the only token guaranteed to differ between two consecutive cards. Their sentences can share a prefix; do not diff text.
- `article[data-answered]` distinguishes "awaiting an answer" from "graded, awaiting Next".
- The Next button is disabled and reads "Loading next card…" for the duration.

So: read `data-card-id`, click, then wait for `aria-busy="false"` **and** a changed `data-card-id`.

**Do not wait on `GET /api/cards/next`.** On the warm path no such request is made for the card you are about to see — it was prefetched earlier. The request you will observe just after an advance is the prefetch for the card *after* the visible one, distinguishable by its `?exclude=` parameter.

**Do not retry a click that appears to do nothing.** It is not doing nothing; it is mid-flight. Next is disabled precisely to stop a retry from posting the same review twice.

### Other things that will surprise you

- **The page mutates on its own.** The stats footer polls every 30 s, changing its own text, `document.title`, and the network log. A change you observe is not necessarily a consequence of your action.
- **Auto-progress.** If the user has enabled it, a correct answer advances on a timer with no click at all. Check whether the footer is `inert` before deciding a Next button is missing.
- **Collapsed accordion sections are not in the DOM.** Settings has six; expand a section before looking for its contents.
- **Dialogs are modal and `aria-hidden` everything else,** including a parent dialog when a confirmation opens on top of it. Re-read the tree after any open or close.
- **The hanja hint is a `note`, named "Hanja hint: …".** It sits in a span positioned absolutely above the blank; before it had a role it was an anonymous text run that merged into the sentence, so it appeared in a raw text dump but not in the tree at all.
- **The grammar-pattern pill is a focusable span, not a button.** Its explanation is carried in `aria-label`, because base-ui tooltips expose no role and unmount their content until hover. It shows under `read_page` with filter `all`, not `interactive`.
- **The hanja reading and gloss are withheld until the card is graded**, matching what is on screen — reading them early would give away the answer.
- **The check button's label is theme-dependent** — "Check", "★ Insert Answer ★", "Draw! (Check)" and others, depending on the selected card theme. Do not match on its text; it is the only button in the card footer before grading.

### Naming conventions

Icon-only controls carry an `aria-label` naming the action and its object ("Delete custom card 자주", "Copy invite code ABC123"). Values that would otherwise be bare — badges, the accuracy figure — carry an `sr-only` prefix naming the dimension they report. Correctness that is conveyed visually by colour is also stated in text, and per-character correctness is exposed as `data-correct` on each character span.

Prefer standard ARIA when adding to this. `data-*` attributes are reserved for things ARIA genuinely cannot express — currently only card identity and per-character correctness.
