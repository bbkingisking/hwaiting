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
  App.tsx                     # Root: Providers + AppHeader + Flashcard
  index.css                   # Tailwind + shadcn theme (CSS variables, light/dark)
  lib/
    constants.ts               # Magic strings, default values, constraints
    types.ts                   # Shared type definitions (Word, Settings, Theme)
    utils.ts                   # cn() helper, color utilities, sentence splitting
    words.ts                   # JSONL parser, getRandomWord()
  components/
    app-header.tsx             # Top bar with settings and theme toggle
    theme-provider.tsx         # React context for light/dark/system theme
    settings-provider.tsx      # React context for user settings
    mode-toggle.tsx            # Sun/Moon/Monitor cycle button
    settings-dialog.tsx        # Settings modal with percentage/threshold controls
    flashcard.tsx              # The flashcard component
    ui/
      button.tsx               # shadcn Button
      card.tsx                 # shadcn Card, CardHeader, CardFooter, etc.
      dialog.tsx               # shadcn Dialog
      field.tsx                # shadcn Field, FieldLabel
      input.tsx                # shadcn Input
      label.tsx                # shadcn Label
      separator.tsx            # shadcn Separator
      slider.tsx               # shadcn Slider
      switch.tsx               # shadcn Switch
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
- Top-right corner: color-coded correct rate percentage (configurable)
- Below the sentence: English hint (word meaning) + English sentence translation
- User types the missing word and presses Enter or clicks Check
- **Correct:** word shown in green, "Correct!" feedback
- **Incorrect:** each character colored per-position (green if matching, red if not), correct answer shown in muted text, notes displayed if any
- Press Enter or Space to advance to next card

### Settings
- Gear icon in top-right opens settings dialog
- Toggle percentage indicator on/off
- Adjust color thresholds with sliders:
  - Red threshold (default: <50%)
  - Yellow threshold (default: <70%)
  - Green for everything above
- Settings persisted to `localStorage` (key: `annyeong-settings`)
- Automatic validation: red threshold always < yellow threshold

### Dark Mode
- Three modes: light / dark / system (follows OS preference)
- Toggle button (Sun/Moon/Monitor icon) in top-right
- Persisted to `localStorage` (key: `theme`)
- CSS variables switch automatically via `.dark` class on `<html>`

## Key Patterns

- **shadcn components** follow base-nova style pattern: `components/ui/*.tsx`, use `cn()` for class merging, `render` prop instead of `asChild`
- **Theming** uses CSS variables defined in `index.css` (`:root` for light, `.dark` for dark), mapped to Tailwind via `@theme inline`
- **Context providers** for theme and settings state management
- **Shared types** centralized in `lib/types.ts`
- **Constants** centralized in `lib/constants.ts` (no magic strings/numbers)
- **Utility functions** for color logic and sentence splitting in `lib/utils.ts`
- **Path alias:** `@/*` maps to `src/*` (configured in both `vite.config.ts` and `tsconfig.json`)
- **Keyboard shortcuts:** Enter to submit/advance, Space to advance
- **No routing** — single-page app, no react-router

## Architecture Decisions

- **Settings storage:** localStorage for quick access, will sync to backend when auth system is added
- **Word statistics:** Extracted from Lingvist export (correctRate, guessCount, wrongGuessCount)
- **Component organization:** kebab-case naming for consistency
- **Type safety:** Shared types prevent duplication, strict TypeScript
- **Validation:** Settings automatically enforce red < yellow thresholds

## Commands

```bash
npm run dev      # Start dev server
npm run build    # TypeScript check + Vite production build
npm run preview  # Preview production build
```
