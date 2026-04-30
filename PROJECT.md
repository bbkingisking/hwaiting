# Annyeong — Korean Flashcard App

## Stack

- **Framework:** React 19 + TypeScript
- **Build:** Vite 8
- **Styling:** Tailwind CSS v4 (CSS-first config, no `tailwind.config.js`)
- **UI Components:** shadcn/ui (base-nova style, `@base-ui/react` primitives)
- **Font:** Geist Variable (via `@fontsource-variable/geist`)
- **Icons:** lucide-react

## Project Structure

```
src/
  main.tsx                    # Entry point
  App.tsx                     # Root: ThemeProvider + ModeToggle + Flashcard
  index.css                   # Tailwind + shadcn theme (CSS variables, light/dark)
  lib/
    utils.ts                  # cn() helper (clsx + tailwind-merge)
    words.ts                  # JSONL parser, Word type, getRandomWord()
  components/
    theme-provider.tsx         # React context for light/dark/system theme
    mode-toggle.tsx            # Sun/Moon/Monitor cycle button
    Flashcard.tsx              # The flashcard component
    ui/
      button.tsx               # shadcn Button
      card.tsx                 # shadcn Card, CardHeader, CardFooter, etc.
      field.tsx                # shadcn Field, FieldLabel
      input.tsx                # shadcn Input
      label.tsx                # shadcn Label
      separator.tsx            # shadcn Separator
words.json                     # Lingvist export — 517 Korean vocab entries (JSONL)
shadcn docs/                   # Reference docs for shadcn components
```

## Data Source

`words.json` is a line-delimited JSON export from Lingvist. Line 1 is metadata; lines 2–518 are vocabulary records.

Each record contains (extracted in `src/lib/words.ts`):
- `form` — the Korean word/phrase (e.g. "자주")
- `hint` — English translation of the word (e.g. "often, frequently")
- `context` — Korean sentence containing the word (e.g. "우리는 자주 만나.")
- `contextTranslation` — English translation of the sentence (e.g. "We meet often.")
- `grammar` — part of speech / conjugation (e.g. "verb, past", "noun", "adverb")
- `politeness` — register (e.g. "informal and polite speech"), if applicable
- `notes` — other comments (cultural notes, usage tips)

## Current Functionality

### Flashcard
- Shows a Korean sentence with the target word replaced by an inline text input
- Above the sentence: grammar and politeness tags (pill-shaped badges)
- Below the sentence: English hint (word meaning) + English sentence translation
- User types the missing word and presses Enter or clicks Check
- **Correct:** word shown in green, "Correct!" feedback
- **Incorrect:** each character colored per-position (green if matching, red if not), correct answer shown in muted text, notes displayed if any
- Enter again or click Next → new random card

### Dark Mode
- Three modes: light / dark / system (follows OS preference)
- Toggle button (Sun/Moon/Monitor icon) fixed top-right
- Persisted to `localStorage`
- CSS variables switch automatically via `.dark` class on `<html>`

## Key Patterns

- **shadcn components** follow the standard pattern: `components/ui/*.tsx`, use `cn()` for class merging, `cva` for variants
- **Theming** uses CSS variables defined in `index.css` (`:root` for light, `.dark` for dark), mapped to Tailwind via `@theme inline`
- **Path alias:** `@/*` maps to `src/*` (configured in both `vite.config.ts` and `tsconfig.json`)
- **No routing** — single-page app, no react-router
- **No state management** — local `useState` only

## Commands

```bash
npm run dev      # Start dev server
npm run build    # TypeScript check + Vite production build
npm run preview  # Preview production build
```
