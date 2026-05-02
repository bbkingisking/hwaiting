# Annyeong — Project Orientation for LLMs

## What is this?

**Annyeong** is a spaced-repetition flashcard web app for learning Korean (and potentially other languages). Users are shown a sentence with a word blanked out and must type the missing word. Cards are scheduled using the **FSRS algorithm** (Free Spaced Repetition Scheduler). The name "annyeong" (안녕) means "hello/goodbye" in Korean.

---

## Architecture Overview

```
annyeong/
├── backend/          # Rust (Axum + SQLx + SQLite)
└── frontend/         # React 19 + TypeScript + Vite + Tailwind v4 + shadcn/ui
```

The backend serves both the API (`/api/*`) and the compiled frontend as static files. There is no separate deployment — it's a single binary that does everything. The frontend calls the backend via relative URLs (`window.location.origin + /api/...`).

---

## Backend

**Stack:** Rust, Axum 0.8, SQLite via SQLx, FSRS 5.2, JWT (HS256), Argon2 password hashing.

**Entry point:** `backend/src/main.rs` — sets up all routes, initializes the DB, and starts the Axum server.

**Environment variables required:**
- `DATABASE_URL` — SQLite path (e.g. `sqlite:./data.db`)
- `JWT_SECRET` — signing key for JWTs (tokens never expire)
- `HOST` and `PORT` — bind address
- `STATIC_DIR` — path to the compiled frontend dist folder
- `ADMIN_USERNAME` and `ADMIN_PASSWORD` — seeded on first startup

### Source modules

| File | Responsibility |
|------|---------------|
| `main.rs` | Router setup, server startup |
| `db.rs` | DB pool init, migrations, admin user seeding, word seeding from `.jsonl` decks |
| `auth.rs` | Login, signup (invite-code gated), JWT generation, `AuthUser` and `AdminUser` extractors |
| `cards.rs` | Core review loop: get next card, submit review (FSRS scheduling), stats, suppress/unsuppress |
| `user.rs` | Profile, language selection, settings CRUD, export/import |
| `custom_cards.rs` | CRUD for user-created cards |
| `admin.rs` | Invite code management (list, generate, delete) |
| `error.rs` | `AppError` enum with `IntoResponse` impl |

### Authentication

All protected endpoints use `AuthUser` as an Axum extractor. It reads the `Authorization: Bearer <token>` header, decodes the JWT, and injects the `user_id: i64`. Admin endpoints additionally use `AdminUser`, which verifies `users.is_admin = true` in the DB.

Signup requires a valid, unused invite code. Invite codes are managed by admins only.

### FSRS Scheduling

On `POST /api/cards/{word_id}/review`:
1. Load `card_states` row for this user+word (if any)
2. Reconstruct `MemoryState { stability, difficulty }` (or treat as new if NULL)
3. Call `fsrs.next_states(memory_state, desired_retention, elapsed_days)`
4. Pick the state for the given rating (1=Again, 3=Good; 2 and 4 supported but not exposed in UI)
5. Compute `due_date = now + interval_days` (minimum 1 day)
6. Upsert `card_states`, insert into `review_history`

The user's `desired_retention` (default 0.9, range 0.5–0.99) is read from `user_settings` and fed into FSRS.

### Card selection logic (`GET /api/cards/next`)

Priority order:
1. **Due cards** first (cards with `due_date <= now`), ordered by `due_date ASC`
2. **New cards** second (no `card_states` row yet)

Filters applied:
- Must match `user.target_language_id`
- Must belong to the user's deck (official cards where `words.user_id IS NULL`, plus the user's own custom cards)
- Not suppressed (`card_states.suppressed != 1`)
- If `suppress_new_cards = true`, new cards are entirely skipped

Optional `?exclude=<word_id>` param lets the client skip the currently-displayed card (used for prefetching).

### Database schema (key tables)

```sql
users           -- id, username, password_hash, is_admin, target_language_id
user_settings   -- user_id, desired_retention, day_boundary_hour, auto_progress_*, show_percentage, thresholds, suppress_new_cards
languages       -- id, code (e.g. 'ko'), name
words           -- id, form, hint, context, context_translation, grammar, politeness, notes (JSON array), language_id, user_id (NULL = official)
card_states     -- user_id, word_id, stability, difficulty, last_review, due_date, suppressed
review_history  -- user_id, word_id, rating, reviewed_at
invite_codes    -- code, used_at, used_by_user_id
```

Migrations live in `backend/migrations/` and run automatically via `sqlx::migrate!` on startup.

### Word seeding

On first startup, `db.rs` reads `backend/decks/*.jsonl`. Each file's name maps to a language code (`korean.jsonl` → `ko`). The JSONL format is nested: `homographs[0].senses[0].contexts[0]` etc. The seeder extracts `form`, `hint` (translation), `context` sentence, `context_translation`, `grammar`, `politeness`, and `notes` (from translation comments).

---

## Frontend

**Stack:** React 19, TypeScript, Vite 8, Tailwind CSS v4, shadcn/ui (Base UI + Radix), Lucide icons, Lottie animations.

**Entry point:** `frontend/src/main.tsx` → `App.tsx`

### Component tree

```
App
├── ThemeProvider         — light/dark/system theme via CSS class on <html>
├── AuthProvider          — JWT stored in localStorage under 'annyeong-token'
└── SettingsProvider      — polls /api/user/settings; exposes settings context
    └── AppContent
        ├── AuthDialog    — login/signup modal (invite code required for signup)
        ├── AppHeader     — nav bar with settings, suppressed cards, custom cards, export/import
        ├── LanguageSelector  — shown once if user has no target_language set
        ├── Flashcard     — the main review card (see below)
        └── StatusIndicator   — bottom bar showing new/due counts, today's stats
```

### Flashcard component (`components/flashcard.tsx`)

The core UX loop:
1. Show sentence with a blank (`<input>`) where the target word goes
2. User types their guess and submits (Enter or "Check" button)
3. Show correct/incorrect feedback; on incorrect, show character-level diff and the correct answer
4. User presses Enter/Space or clicks "Next" → `onReview(rating)` fires
5. **Auto-progress mode:** if `autoProgressOnCorrect` is enabled, correct answers skip step 3/4 and advance after `autoProgressDelay` ms

Rating values: `1` = Again (wrong), `3` = Good (correct).

### Prefetch strategy (`App.tsx`)

To make card transitions feel instant, `App.tsx` starts a background fetch for the *next* card as soon as the current one is displayed. The prefetch is held in a `useRef<PrefetchSlot>` (not state, to avoid re-renders). Key details:
- The prefetch passes `?exclude=<current_word_id>` so the server doesn't return the card the user is currently looking at.
- The review submission (`submitReview`) is a fire-and-forget promise. The *next next* prefetch (N+2) waits on this promise before firing, to avoid the DB returning the just-reviewed card again before its `due_date` is updated.
- If the prefetch is already resolved when the user advances, the transition is synchronous (zero loading state).

### API layer (`frontend/src/lib/api.ts`)

All API calls go through `fetchWithAuth()`, which reads the JWT from `localStorage` and attaches `Authorization: Bearer <token>`. Throws `ApiError(status, message)` on non-2xx responses. All functions return typed interfaces that mirror the backend's `Serialize` structs.

### User settings

`SettingsProvider` loads settings from the API on mount and makes them available via `useSettings()`. Settings include:
- `showPercentage` — show FSRS difficulty score on cards
- `redThreshold` / `yellowThreshold` — score color thresholds  
- `dayBoundaryHour` — when "today" resets (default 4am, so late-night reviews count for that day)
- `autoProgressOnCorrect` + `autoProgressDelay` — skip the feedback screen on correct answers
- `suppressNewCards` — stop introducing new cards until backlog is cleared
- `desiredRetention` — FSRS target retention (0.5–0.99)

---

## Key design decisions to be aware of

1. **JWT tokens never expire.** The server skips `exp` validation entirely. Logging out just removes the token from `localStorage`.

2. **Invite-only signup.** There is no public registration. Admins generate invite codes via `/api/admin/invites`.

3. **Suppress vs. ignore.** "Suppressing" a card sets `card_states.suppressed = 1`. It still appears in the DB and can be unsuppressed from the UI (AppHeader → suppressed cards dialog). It's different from `suppress_new_cards`, which is a setting that hides *all* unseen cards until the review queue is empty.

4. **Custom cards share the `words` table.** Custom cards have `words.user_id = <user_id>`. Official deck cards have `words.user_id = NULL`. The card selection query uses `(w.user_id IS NULL OR w.user_id = ?)` to show both.

5. **Import matches cards by content, not ID.** The import endpoint finds words by `(form, hint, context, language_id)` — not by `word_id` — because IDs can differ between database instances.

6. **Optimistic review UI.** The frontend advances to the next card immediately on rating; the `submitReview` call happens in the background. If it fails, an error banner appears but the UI does not roll back.

7. **Single binary deployment.** The Rust binary serves both `/api/*` routes and the frontend static files from `STATIC_DIR`. SPA routing is handled by falling back to `index.html` for unknown paths.

---

## Where to look for specific things

| I want to... | Look here |
|---|---|
| Change card scheduling | `backend/src/cards.rs` → `submit_review` |
| Add a new API endpoint | `backend/src/main.rs` (register route) + new handler in appropriate module |
| Change what the card UI looks like | `frontend/src/components/flashcard.tsx` |
| Add a new user setting | `backend/src/user.rs` + `user_settings` table migration + `frontend/src/lib/api.ts` + `SettingsProvider` |
| Add a new language/deck | Drop a `<language>.jsonl` in `backend/decks/` and add a code mapping in `db.rs::seed_words_if_empty` + add the language to the `languages` table via migration |
| Modify the DB schema | Add a new file in `backend/migrations/` with the next timestamp prefix |
| Change auth logic | `backend/src/auth.rs` |
| Manage admin features | `backend/src/admin.rs` |
