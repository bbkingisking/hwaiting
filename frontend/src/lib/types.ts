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
  difficulty: number | null
  guess_count: number
  wrong_guess_count: number
  hanja_hints: HanjaHint[]
}

// Theme types
export type Theme = 'light' | 'dark' | 'system'

// Color rate types
export type RateColor = 'text-destructive' | 'text-yellow-600 dark:text-yellow-500' | 'text-green-600 dark:text-green-500'