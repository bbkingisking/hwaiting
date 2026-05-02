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

// Split sentence into before/target/after parts
export function splitSentence(sentence: string, target: string): { before: string; after: string } {
  const idx = sentence.indexOf(target)
  if (idx < 0) {
    return { before: sentence, after: '' }
  }
  return {
    before: sentence.slice(0, idx),
    after: sentence.slice(idx + target.length),
  }
}

// Format time until a due date in a human-readable format
export function formatTimeUntil(isoTimestamp: string): string | null {
  const now = new Date()
  const due = new Date(isoTimestamp)
  const diffMs = due.getTime() - now.getTime()
  if (diffMs <= 0) return null
  const diffMinutes = Math.floor(diffMs / 60000)
  const hours = Math.floor(diffMinutes / 60)
  const minutes = diffMinutes % 60
  if (hours > 0) return `${hours}h ${minutes}m`
  return `${minutes}m`
}
