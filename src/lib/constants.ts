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
} as const

// Color thresholds
export const THRESHOLD_CONSTRAINTS = {
  MIN: 0,
  MAX: 100,
  STEP: 5,
} as const