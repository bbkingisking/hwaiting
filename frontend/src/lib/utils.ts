import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"
import type { Settings } from './types'
import { 
  POS_LABELS, 
  SPEECH_LEVEL_LABELS, 
  TENSE_LABELS, 
  ORIGIN_TYPE_LABELS, 
  GRADE_LABELS 
} from './constants'

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

// Get English label for part of speech
export function getPosLabel(pos: string | null): string | null {
  if (!pos) return null
  return POS_LABELS[pos] || pos
}

// Get English label for speech level
export function getSpeechLevelLabel(speechLevel: string | null): string | null {
  if (!speechLevel) return null
  return SPEECH_LEVEL_LABELS[speechLevel] || speechLevel
}

// Get English label for tense
export function getTenseLabel(tense: string | null): string | null {
  if (!tense) return null
  return TENSE_LABELS[tense] || tense
}

// Get English label for origin type
export function getOriginTypeLabel(originType: string | null): string | null {
  if (!originType) return null
  return ORIGIN_TYPE_LABELS[originType] || originType
}

// Get English label for grade/level
export function getGradeLabel(grade: string | null): string | null {
  if (!grade) return null
  return GRADE_LABELS[grade] || grade
}
