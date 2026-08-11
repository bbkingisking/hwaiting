import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"
import type { Settings } from './types'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

// Get color class based on percentage and thresholds
export function getPercentageColor(percentage: number, settings: Settings): string {
  if (percentage < settings.redThreshold) return 'text-destructive'
  if (percentage < settings.yellowThreshold) return 'text-yellow-600 dark:text-yellow-500'
  return 'text-green-600 dark:text-green-500'
}

// Get color class based on difficulty (1-10 scale from FSRS)
export function getDifficultyColor(difficulty: number): string {
  if (difficulty >= 7) return 'bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400'
  if (difficulty >= 4) return 'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400'
  return 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400'
}

// Build a link to a word's entry in KRDICT, the dictionary the starter cards
// were derived from. `krdict_id` is KRDICT's own `ParaWordNo`, so the entry
// resolves directly with no search step. Returns null for cards with no
// KRDICT origin (user-created custom cards), so callers can skip the link.
// The language segment (/eng/) is required - the path without one 404s.
export function krdictUrl(krdictId: number | null | undefined): string | null {
  if (krdictId == null) return null
  return `https://krdict.korean.go.kr/eng/dicSearch/SearchView?ParaWordNo=${krdictId}`
}

// Format time until a due date in a human-readable format
export function formatTimeUntil(isoTimestamp: string): string | null {
  // Zone-less SQLite datetimes ("YYYY-MM-DD HH:MM:SS") are UTC; without the
  // T/Z normalization, browsers parse them as local time (or reject them).
  const normalized = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(isoTimestamp)
    ? isoTimestamp.replace(' ', 'T') + 'Z'
    : isoTimestamp
  const now = new Date()
  const due = new Date(normalized)
  if (Number.isNaN(due.getTime())) return null
  const diffMs = due.getTime() - now.getTime()
  if (diffMs <= 0) return null
  const diffMinutes = Math.floor(diffMs / 60000)
  const hours = Math.floor(diffMinutes / 60)
  const minutes = diffMinutes % 60
  if (hours > 0) return `${hours}h ${minutes}m`
  return `${minutes}m`
}

