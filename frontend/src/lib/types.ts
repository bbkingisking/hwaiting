import type { components } from './api-schema'

// Settings types
export interface Settings {
  showPercentage: boolean
  redThreshold: number
  yellowThreshold: number
  dayBoundaryHour: number
  autoProgressOnCorrect: boolean
  autoProgressDelay: number
  desiredRetention: number
  dailyNewCardLimit: number
  historyColorizedArea: boolean
  historyColoredDots: boolean
  historyThresholdLines: boolean
  hasFsrsParameters: boolean
}

// Word types
export interface Word {
  form: string
  hint: string
  context: string
  contextTranslation: string
  grammar: string | null
  politeness: string | null
  notes: string[]
  correctRate: number
  guessCount: number
  wrongGuessCount: number
}

// Hanja hint and card types are generated from the backend's OpenAPI spec
// (see scripts/generate-api-types.sh) rather than hand-typed - this used to
// be an independent hand-written mirror of the same shape api.ts also
// hand-typed, which is exactly the kind of duplication that let the two
// drift apart.
export type HanjaHint = components['schemas']['HanjaHint']

// What's known about the card being reviewed before an answer is checked -
// everything needed to render the sentence-with-blank, badges, and both
// translations, but nothing `target` could be inferred from.
export type CardPrompt = components['schemas']['NextCardResponse']

// Disclosed by POST /api/cards/{id}/check once an attempt has been graded:
// the answer, the word's citation form, the hanja reading/gloss, and the
// grammar pattern's conjugation endings - everything CardPrompt withheld.
//
// Deliberately NOT merged into a single `CardPrompt & Partial<CardReveal>`
// type held in App.tsx's `card` state, as an earlier version of this file
// did: that merge happened via a second, independently-timed setState call
// (App.tsx's, following the network response) racing against the setState
// that flips Flashcard's own `answered` to true (Flashcard's, following the
// same response) - nothing guarantees the parent's update is visible in the
// child's props by the time the child re-renders on `answered`, and in
// production it wasn't always, i.e. `card.target` could still be `undefined`
// the moment the per-character diff tried to index into it. Flashcard now
// holds its own `reveal: CardReveal | null` state, set in the same
// synchronous continuation as `answered`/`correct` - no cross-component
// timing involved. See flashcard.tsx.
export type CardReveal = components['schemas']['CardReveal']

// EditCardDialog edits the backend's canonical full-card shape
// (api.ts's AdminCard) directly - see its doc comment for why the review
// flow needs an adapter (toAdminCard in flashcard.tsx) rather than passing
// CardPrompt/CardReveal straight through.

// Theme types
export type Theme = 'light' | 'dark' | 'system'

// Color rate types
export type RateColor = 'text-destructive' | 'text-yellow-600 dark:text-yellow-500' | 'text-green-600 dark:text-green-500'