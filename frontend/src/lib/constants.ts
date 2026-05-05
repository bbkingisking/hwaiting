// Keyboard keys
export const KEYS = {
  ENTER: 'Enter',
  SPACE: ' ',
} as const

// Local storage keys
export const STORAGE_KEYS = {
  SETTINGS: 'annyeong-settings',
  THEME: 'theme',
} as const

// Default settings values
export const DEFAULT_SETTINGS = {
  SHOW_PERCENTAGE: true,
  RED_THRESHOLD: 50,
  YELLOW_THRESHOLD: 70,
  DAY_BOUNDARY_HOUR: 4,
  AUTO_PROGRESS_ON_CORRECT: false,
  AUTO_PROGRESS_DELAY: 1.5,
  DESIRED_RETENTION: 0.9,
  DAILY_NEW_CARD_LIMIT: 20,
} as const

// Desired retention constraints
export const DESIRED_RETENTION_CONSTRAINTS = {
  MIN: 0.5,
  MAX: 0.99,
  STEP: 0.01,
} as const

// Color thresholds
export const THRESHOLD_CONSTRAINTS = {
  MIN: 0,
  MAX: 100,
  STEP: 5,
} as const

// Auto-progress delay constraints (in milliseconds)
export const AUTO_PROGRESS_DELAY_CONSTRAINTS = {
  MIN: 0,
  MAX: 3000,
  STEP: 100,
} as const

// Korean to English mapping tables for display
export const POS_LABELS: Record<string, string> = {
  '동사': 'Verb',
  '명사': 'Noun',
  '형용사': 'Adjective',
  '부사': 'Adverb',
  '의존 명사': 'Bound Noun',
  '대명사': 'Pronoun',
  '수사': 'Numeral',
  '감탄사': 'Interjection',
  '관형사': 'Determiner',
  '보조 형용사': 'Auxiliary Adjective',
  '보조 동사': 'Auxiliary Verb',
  '조사': 'Particle',
  '품사 없음': 'No POS',
} as const

export const SPEECH_LEVEL_LABELS: Record<string, string> = {
  'hae-che': 'Intimate (해체)',
  'haeyo-che': 'Polite Informal (해요체)',
  'hasipsio-che': 'Formal (하십시오체)',
  'haera-che': 'Plain (해라체)',
  'hao-che': 'Semi-Formal (하오체)',
  'hage-che': 'Semi-Plain (하게체)',
} as const

export const TENSE_LABELS: Record<string, string> = {
  'present': 'Present',
  'past': 'Past',
  'future': 'Future',
  'progressive': 'Progressive',
  'retrospective': 'Retrospective',
} as const

export const ORIGIN_TYPE_LABELS: Record<string, string> = {
  '고유어': 'Native Korean',
  '한자어': 'Sino-Korean',
  '외래어': 'Loanword',
  '혼종어': 'Hybrid',
} as const

export const GRADE_LABELS: Record<string, string> = {
  '초급': 'Beginner',
  '중급': 'Intermediate',
  '고급': 'Advanced',
} as const
