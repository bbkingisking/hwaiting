// Settings types
export interface Settings {
  showPercentage: boolean
  redThreshold: number
  yellowThreshold: number
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

// Theme types
export type Theme = 'light' | 'dark' | 'system'

// Color rate types
export type RateColor = 'text-destructive' | 'text-yellow-600 dark:text-yellow-500' | 'text-green-600 dark:text-green-500'