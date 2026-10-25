import { useEffect, useEffectEvent, useState } from 'react'
import { getStats } from '@/lib/api'
import { cn, getPercentageColor } from '@/lib/utils'
import { useSettings } from '@/components/settings-provider'
import { useTimeUntil } from '@/hooks/use-time-until'
import { parseDueAt, scheduleReviewRefresh } from '@/lib/review-timing'

export function StatusIndicator({ nextCardDueAt = null, onCardsAvailable }: {
  nextCardDueAt?: string | null
  onCardsAvailable?: () => void
}) {
  const { settings } = useSettings()
  const [dueCount, setDueCount] = useState<number | null>(null)
  const [percentage, setPercentage] = useState<number | null>(null)
  const [nextDueAt, setNextDueAt] = useState<string | null>(null)
  const [newTodayCount, setNewTodayCount] = useState<number | null>(null)
  const remaining = useTimeUntil(dueCount === 0 ? nextCardDueAt ?? nextDueAt : null)

  const fetchStats = useEffectEvent(async () => {
    try {
      const stats = await getStats()
      setDueCount(stats.due_count)
      setPercentage(stats.percentage ?? null)
      setNextDueAt(stats.next_due_at ?? null)
      setNewTodayCount(stats.new_today_count)
    } catch (err) {
      console.error('Failed to fetch stats:', err)
    }
  })

  useEffect(() => {
    fetchStats()
  }, [])

  // The empty-state deadline also accounts for the daily new-card reset.
  const deadline = [nextCardDueAt, nextDueAt]
    .filter((value): value is string => value !== null && Number.isFinite(parseDueAt(value)))
    .sort((a, b) => parseDueAt(a) - parseDueAt(b))[0] ?? null
  const refresh = useEffectEvent(() => {
    fetchStats()
    onCardsAvailable?.()
  })

  useEffect(() => scheduleReviewRefresh(deadline, refresh), [deadline])

  // Update tab title with due count
  useEffect(() => {
    if (dueCount !== null) {
      document.title = dueCount > 0 ? `(${dueCount}) Hwaiting` : 'Hwaiting'
    }
  }, [dueCount])

  return (
    <footer
      aria-label="Review stats"
      className="fixed bottom-4 left-1/2 -translate-x-1/2 flex items-center gap-3 text-sm text-muted-foreground"
    >
      {dueCount !== null && (
        <span>
          Due: {dueCount}
          {remaining && <span className="ml-1">({remaining})</span>}
        </span>
      )}
      {newTodayCount !== null && (
        <span>
          New today: {newTodayCount}/{settings.dailyNewCardLimit}
        </span>
      )}
      <span className={cn(percentage !== null && getPercentageColor(percentage, settings))}>
        <span className="sr-only">Overall accuracy: </span>
        {percentage !== null ? `${percentage}%` : '—'}
      </span>
    </footer>
  )
}
