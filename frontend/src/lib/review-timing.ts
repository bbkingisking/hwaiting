// SQLite's zone-less datetime strings represent UTC.
export function parseDueAt(timestamp: string): number {
  const normalized = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(timestamp)
    ? timestamp.replace(' ', 'T') + 'Z'
    : timestamp
  return Date.parse(normalized)
}

// Refresh at the deadline, or on return if the deadline has passed. Never poll.
// Hidden tabs wait until visible, since their timers may be suspended anyway.
export function scheduleReviewRefresh(timestamp: string | null, refresh: () => void): () => void {
  const dueAt = timestamp ? parseDueAt(timestamp) : NaN
  let timer: ReturnType<typeof setTimeout> | undefined
  let lastRefresh = -Infinity

  const refreshVisible = () => {
    if (document.visibilityState === 'hidden') return
    const now = Date.now()
    if (!Number.isFinite(dueAt) || now < dueAt + 1000) return
    // Focus, visibilitychange and a suspended timer can all fire together.
    if (now - lastRefresh < 1000) return
    lastRefresh = now
    refresh()
  }

  const schedule = () => {
    if (!Number.isFinite(dueAt)) return
    // Allow the server to cross the due second before asking for the card.
    const remaining = dueAt + 1000 - Date.now()
    timer = setTimeout(() => {
      if (Date.now() < dueAt + 1000) schedule()
      else refreshVisible()
    }, Math.min(Math.max(remaining, 0), 2 ** 31 - 1))
  }

  schedule()
  window.addEventListener('focus', refreshVisible)
  document.addEventListener('visibilitychange', refreshVisible)
  return () => {
    clearTimeout(timer)
    window.removeEventListener('focus', refreshVisible)
    document.removeEventListener('visibilitychange', refreshVisible)
  }
}
