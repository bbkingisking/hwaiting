import { createRefreshTask } from '@/lib/refresh-task'
import { useEffect, useEffectEvent, useRef, useState } from 'react'
import { getTodayStats } from '@/lib/api'
import { cn, getPercentageColor } from '@/lib/utils'
import { useSettings } from '@/components/settings-provider'
import { useTimeUntil } from '@/hooks/use-time-until'
import { scheduleReviewRefresh } from '@/lib/review-timing'
import { Percentage } from '@/components/percentage'

export function StatusIndicator({ refreshKey = 0, availableCount = null, nextAvailableAt = null, onCardsAvailable }: {
  availableCount?: number | null
  refreshKey?: number
  nextAvailableAt?: string | null
  onCardsAvailable?: () => Promise<boolean>
}) {
  const { settings } = useSettings()
  const requestVersion = useRef(0)
  const taskRef = useRef<ReturnType<typeof createRefreshTask> | null>(null)
  const [dayEndsAt, setDayEndsAt] = useState<string | null>(null)
  const [percentage, setPercentage] = useState<number | null>(null)
  const [newTodayCount, setNewTodayCount] = useState<number | null>(null)
  const remaining = useTimeUntil(nextAvailableAt)

  const refresh = useEffectEvent(async (refreshQueue: boolean, current: () => boolean = () => true) => {
    const version = ++requestVersion.current
    try {
      const stats = await getTodayStats()
      // A failed queue read leaves its availability deadline pending for retry.
      if (refreshQueue && await onCardsAvailable?.() === false) return false
      if (!current() || version !== requestVersion.current) return true
      setDayEndsAt(stats.day_ends_at)
      setPercentage(stats.percentage ?? null)
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

  useEffect(() => scheduleReviewRefresh(nextAvailableAt, () => refresh(true)), [nextAvailableAt])

  useEffect(() => scheduleReviewRefresh(dayEndsAt, () => refresh(false)), [dayEndsAt])

  useEffect(() => {
    document.title = availableCount ? `(${availableCount}) Hwaiting` : 'Hwaiting'
    return () => { document.title = 'Hwaiting' }
  }, [availableCount])

  return (
    <footer
      aria-label="Review stats"
      className="flex flex-wrap items-center justify-center gap-x-3 gap-y-1 text-sm text-muted-foreground lg:col-start-2 lg:row-start-1"
    >
      {availableCount !== null && <span>Available: {availableCount}</span>}
      {remaining && <span>Next in {remaining}</span>}
      {newTodayCount !== null && (
        <span>
          New today: {newTodayCount}/{settings.dailyNewCardLimit}
        </span>
      )}
      <span className={cn(percentage !== null && getPercentageColor(percentage, settings))}>
        <span className="sr-only">Today’s accuracy: </span>
        {percentage !== null ? (
          <Percentage value={percentage} />
        ) : '—'}
      </span>
    </footer>
  )
}
