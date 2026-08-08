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

// Card type from backend (includes stats and card_id)
export type Card = components['schemas']['NextCardResponse']

// Subset of Card that the edit dialog needs (also matches admin card search results)
export type EditableCard = Pick<
  Card,
  | 'card_id'
  | 'krdict_id'
  | 'word'
  | 'definition'
  | 'pos'
  | 'origin_type'
  | 'hanja'
  | 'hanja_eum'
  | 'grade'
  | 'trans_word'
  | 'trans_dfn'
  | 'sentence'
  | 'sentence_translation'
  | 'target'
  | 'alternatives'
  | 'speech_level'
  | 'tense'
  | 'grammar_pattern'
>

// Theme types
export type Theme = 'light' | 'dark' | 'system'

// Color rate types
export type RateColor = 'text-destructive' | 'text-yellow-600 dark:text-yellow-500' | 'text-green-600 dark:text-green-500'