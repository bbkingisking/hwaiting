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
