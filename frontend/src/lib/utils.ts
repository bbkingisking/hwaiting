import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"
import type { Settings } from './types'
import { parseDueAt } from './review-timing'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

// Get color class based on percentage and thresholds
export function getPercentageColor(percentage: number, settings: Settings): string {
  if (percentage < settings.redThreshold) return 'text-destructive'
  if (percentage < settings.yellowThreshold) return 'text-yellow-600 dark:text-yellow-500'
  return 'text-green-600 dark:text-green-500'
}

// Build a link to a word's entry in KRDICT, the dictionary the starter cards
// were derived from. `krdict_id` is KRDICT's own `ParaWordNo`, so the entry
// resolves directly with no search step. Returns null for cards with no
// KRDICT origin, so callers can skip the link.
// The language segment (/eng/) is required - the path without one 404s.
export function krdictUrl(krdictId: number | null | undefined): string | null {
  if (krdictId == null) return null
  return `https://krdict.korean.go.kr/eng/dicSearch/SearchView?ParaWordNo=${krdictId}`
}

// Format time until a due date in a human-readable format
export function formatTimeUntil(isoTimestamp: string): string | null {
  const dueAt = parseDueAt(isoTimestamp)
  if (!Number.isFinite(dueAt)) return null
  const diffMs = dueAt - Date.now()
  if (diffMs <= 0) return null
  if (diffMs < 60000) return '<1m'
  const diffMinutes = Math.floor(diffMs / 60000)
  const hours = Math.floor(diffMinutes / 60)
  const minutes = diffMinutes % 60
  if (hours > 0) return `${hours}h ${minutes}m`
  return `${minutes}m`
}

