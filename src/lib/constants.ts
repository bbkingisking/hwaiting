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
  AUTO_PROGRESS_ON_CORRECT: true,
  AUTO_PROGRESS_DELAY: 0,
  SUPPRESS_NEW_CARDS: false,
  DESIRED_RETENTION: 0.9,
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