import { useEffect, useRef, useState } from 'react'
import { getStats } from '@/lib/api'
import { cn, formatTimeUntil, getPercentageColor } from '@/lib/utils'
import { useSettings } from '@/components/settings-provider'

export function StatusIndicator({ onCardsAvailable }: { onCardsAvailable?: () => void }) {
  const { settings } = useSettings()
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
      <span className={cn(percentage !== null && getPercentageColor(percentage, settings))}>
        {percentage !== null ? `${percentage}%` : '—'}
      </span>
    </div>
  )
}