import { useEffect, useState } from 'react'
import { formatTimeUntil } from '@/lib/utils'
import { parseDueAt } from '@/lib/review-timing'

// Only the label ticks; no requests are made as time passes.
export function useTimeUntil(timestamp: string | null): string | null {
  const [, tick] = useState(0)

  useEffect(() => {
    if (!timestamp) return
    const dueAt = parseDueAt(timestamp)
    let timer: ReturnType<typeof setTimeout> | undefined
    const update = () => {
      clearTimeout(timer)
      tick(value => value + 1)
      const remaining = dueAt - Date.now()
      if (remaining > 0 && document.visibilityState !== 'hidden') {
        // Wake at the next minute-label change, or when the card is due.
        timer = setTimeout(update, remaining < 60000 ? remaining : remaining % 60000 + 1)
      }
    }
    const onVisible = () => {
      if (document.visibilityState === 'visible') update()
    }
    update()
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      clearTimeout(timer)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [timestamp])

  return timestamp ? formatTimeUntil(timestamp) : null
}
