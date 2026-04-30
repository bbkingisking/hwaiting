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
