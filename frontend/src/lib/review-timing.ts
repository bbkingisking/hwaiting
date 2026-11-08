// SQLite's zone-less datetime strings represent UTC.
export function parseDueAt(timestamp: string): number {
  const normalized = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(timestamp)
    ? timestamp.replace(' ', 'T') + 'Z'
    : timestamp
  return Date.parse(normalized)
}

// Refresh successfully once per deadline, retrying only failed requests.
// Background timers may be delayed; returning to the tab is a fallback.
export function scheduleReviewRefresh(
  timestamp: string | null,
  refresh: () => boolean | void | Promise<boolean | void>,
): () => void {
  const dueAt = timestamp ? parseDueAt(timestamp) : NaN
  let timer: ReturnType<typeof setTimeout> | undefined
  let refreshed = false
  let inFlight = false
  let disposed = false
  let retryAt = 0
  let retryDelay = 1000

  const refreshIfDue = async () => {
    const now = Date.now()
    if (disposed || refreshed || inFlight || !Number.isFinite(dueAt)
      || now < dueAt + 1000 || now < retryAt) return
    // The timer and return events share a single in-flight refresh.
    inFlight = true
    clearTimeout(timer)
    try {
      const result = await refresh()
      if (result !== false) refreshed = true
    } catch {
      // A rejected request leaves the deadline pending for the next attempt.
    } finally {
      inFlight = false
      if (!disposed && !refreshed) {
        retryAt = Date.now() + retryDelay
        timer = setTimeout(refreshIfDue, retryDelay)
        retryDelay = Math.min(retryDelay * 2, 60000)
      }
    }
  }

  const refreshVisible = () => {
    if (document.visibilityState !== 'hidden') refreshIfDue()
  }

  const schedule = () => {
    if (!Number.isFinite(dueAt)) return
    // Allow the server to cross the due second before asking for the card.
    const remaining = dueAt + 1000 - Date.now()
    timer = setTimeout(() => {
      if (Date.now() < dueAt + 1000) schedule()
      else refreshIfDue()
    }, Math.min(Math.max(remaining, 0), 2 ** 31 - 1))
  }

  schedule()
  window.addEventListener('focus', refreshVisible)
  document.addEventListener('visibilitychange', refreshVisible)
  return () => {
    disposed = true
    clearTimeout(timer)
    window.removeEventListener('focus', refreshVisible)
    document.removeEventListener('visibilitychange', refreshVisible)
  }
}
