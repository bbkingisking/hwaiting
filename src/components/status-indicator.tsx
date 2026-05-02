import { useEffect, useRef, useState } from 'react'
import { getStats } from '@/lib/api'
import { cn } from '@/lib/utils'

export function StatusIndicator({ onCardsAvailable }: { onCardsAvailable?: () => void }) {
  const [dueCount, setDueCount] = useState<number | null>(null)
  const [percentage, setPercentage] = useState<number | null>(null)
  const [nextDueAt, setNextDueAt] = useState<string | null>(null)
  const prevDueRef = useRef(dueCount)

  useEffect(() => {
    fetchStats()
    // Refresh stats every 30 seconds
    const interval = setInterval(fetchStats, 30000)
    return () => clearInterval(interval)
  }, [])

  // Notify parent when cards transition from 0 to due
  useEffect(() => {
    if (prevDueRef.current !== null && prevDueRef.current === 0 && dueCount !== null && dueCount > 0) {
      onCardsAvailable?.()
    }
    prevDueRef.current = dueCount
  }, [dueCount, onCardsAvailable])

  const fetchStats = async () => {
    try {
      const stats = await getStats()
      setDueCount(stats.due_count)
      setPercentage(stats.percentage)
      setNextDueAt(stats.next_due_at)
    } catch (err) {
      console.error('Failed to fetch stats:', err)
    }
  }

  const formatTimeUntil = (isoTimestamp: string): string | null => {
    const now = new Date()
    const due = new Date(isoTimestamp)
    const diffMs = due.getTime() - now.getTime()
    
    // Don't show if already due (edge case during refresh)
    if (diffMs <= 0) {
      return null
    }
    
    const diffMinutes = Math.floor(diffMs / 60000)
    const hours = Math.floor(diffMinutes / 60)
    const minutes = diffMinutes % 60
    
    if (hours > 0) {
      return `${hours}h${minutes}m`
    } else {
      return `${minutes}m`
    }
  }

  const getPercentageColor = () => {
    if (percentage === null) return ''
    if (percentage >= 70) return 'text-green-600 dark:text-green-500'
    if (percentage >= 50) return 'text-yellow-600 dark:text-yellow-500'
    return 'text-destructive'
  }

  return (
    <div className="fixed bottom-4 left-1/2 -translate-x-1/2 flex items-center gap-3 text-sm text-muted-foreground">
      {dueCount !== null && (
        <span>
          Due: {dueCount}
          {(() => {
            if (dueCount === 0 && nextDueAt) {
              const formatted = formatTimeUntil(nextDueAt)
              if (formatted) {
                return <span className="ml-1">({formatted})</span>
              }
            }
            return null
          })()}
        </span>
      )}
      <span className={cn(percentage !== null && getPercentageColor())}>
        {percentage !== null ? `${percentage}%` : '—'}
      </span>
    </div>
  )
}