// Keyboard keys
export const KEYS = {
  ENTER: 'Enter',
  SPACE: ' ',
} as const

// Local storage keys
export const STORAGE_KEYS = {
  SETTINGS: 'annyeong-settings',
  THEME: 'theme',
  CARD_THEME: 'annyeong-card-theme',
  DEBUG_STATUS_BAR: 'annyeong-debug-status-bar',
} as const

// Default settings values
export const DEFAULT_SETTINGS = {
  SHOW_PERCENTAGE: true,
  RED_THRESHOLD: 50,
  YELLOW_THRESHOLD: 70,
  DAY_BOUNDARY_HOUR: 4,
  AUTO_PROGRESS_ON_CORRECT: false,
  // Milliseconds, matching AUTO_PROGRESS_DELAY_CONSTRAINTS below
  AUTO_PROGRESS_DELAY: 1500,
  DESIRED_RETENTION: 0.9,
  DAILY_NEW_CARD_LIMIT: 20,
  HISTORY_COLORIZED_AREA: false,
  HISTORY_COLORED_DOTS: false,
  HISTORY_THRESHOLD_LINES: false,
  // Unlike the rest of Settings, this is never sent to the backend - it's a
  // device-local debug preference, same as card theme (see
  // card-theme-provider.tsx). Kept in settings-provider.tsx anyway so it
  // shows up through the same useSettings()/updateSettings() surface
  // everything else in the settings dialog already uses.
  DEBUG_STATUS_BAR: false,
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

