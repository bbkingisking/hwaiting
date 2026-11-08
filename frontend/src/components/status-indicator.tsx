import { createRefreshTask } from '@/lib/refresh-task'
import { useEffect, useEffectEvent, useRef, useState } from 'react'
import { getStats } from '@/lib/api'
import { cn, getPercentageColor } from '@/lib/utils'
import { useSettings } from '@/components/settings-provider'
import { useTimeUntil } from '@/hooks/use-time-until'
import { parseDueAt, scheduleReviewRefresh } from '@/lib/review-timing'
import { Percentage } from '@/components/percentage'

export function StatusIndicator({ refreshKey = 0, nextCardDueAt = null, onCardsAvailable }: {
  refreshKey?: number
  nextCardDueAt?: string | null
  onCardsAvailable?: () => Promise<boolean>
}) {
  const { settings } = useSettings()
  const requestVersion = useRef(0)
  const taskRef = useRef<ReturnType<typeof createRefreshTask> | null>(null)
  const [dueCount, setDueCount] = useState<number | null>(null)
  const [percentage, setPercentage] = useState<number | null>(null)
  const [nextDueAt, setNextDueAt] = useState<string | null>(null)
  const [newTodayCount, setNewTodayCount] = useState<number | null>(null)
  const remaining = useTimeUntil(dueCount === 0 ? nextCardDueAt ?? nextDueAt : null)

  const refresh = useEffectEvent(async (refreshQueue: boolean, current: () => boolean = () => true) => {
    const version = ++requestVersion.current
    try {
      const stats = await getStats()
      // Keep the old deadline until both reads succeed so a failed card
      // request can retry it, even if stats already know a later due date.
      if (refreshQueue && await onCardsAvailable?.() === false) return false
      if (!current() || version !== requestVersion.current) return true
      setDueCount(stats.due_count)
      setPercentage(stats.percentage ?? null)
      setNextDueAt(stats.next_due_at ?? null)
      setNewTodayCount(stats.new_today_count)
      return true
    } catch (err) {
      console.error('Failed to fetch stats:', err)
      return false
    }
  })

  useEffect(() => {
    const task = createRefreshTask(async current => {
      if (!await refresh(false, current)) throw new Error('Stats refresh failed')
    })
    taskRef.current = task
    return () => { task.dispose(); ++requestVersion.current }
  }, [])
  useEffect(() => { taskRef.current?.request() }, [refreshKey])

  // The empty-state deadline also accounts for the daily new-card reset.
  const deadline = [nextCardDueAt, nextDueAt]
    .filter((value): value is string => value !== null && Number.isFinite(parseDueAt(value)))
    .sort((a, b) => parseDueAt(a) - parseDueAt(b))[0] ?? null
  useEffect(() => scheduleReviewRefresh(deadline, () => refresh(true)), [deadline])

  // Update tab title with due count
  useEffect(() => {
    if (dueCount !== null) {
      document.title = dueCount > 0 ? `(${dueCount}) Hwaiting` : 'Hwaiting'
    }
  }, [dueCount])

  return (
    <footer
      aria-label="Review stats"
      className="flex flex-wrap items-center justify-center gap-x-3 gap-y-1 text-sm text-muted-foreground lg:col-start-2 lg:row-start-1"
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
        {percentage !== null ? (
          <Percentage value={percentage} />
        ) : '—'}
      </span>
    </footer>
  )
}
