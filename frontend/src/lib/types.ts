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

// Hanja hint type
export interface HanjaHint {
  hanja: string
  hanja_eum: string | null
  trans_word: string | null
}

// Card type from backend (includes stats and card_id)
export interface Card {
  card_id: number
  word: string
  definition: string | null
  pos: string | null
  origin_type: string | null
  hanja: string | null
  hanja_eum: string | null
  grade: string | null
  trans_word: string
  trans_dfn: string | null
  sentence: string
  sentence_translation: string
  target: string
  alternatives: string[]
  speech_level: string | null
  tense: string | null
  grammar_pattern: string | null
  difficulty: number | null
  guess_count: number
  wrong_guess_count: number
  hanja_hints: HanjaHint[]
}

// Subset of Card that the edit dialog needs (also matches admin card search results)
export type EditableCard = Pick<
  Card,
  | 'card_id'
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