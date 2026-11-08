// Serialise invalidations, discard superseded results, retry transient failures.
// A request arriving during a read schedules another read, never gets lost.
export function createRefreshTask(run: (current: () => boolean) => Promise<void>) {
  let version = 0
  let running = false
  let stopped = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let delay = 1000
  const work = async () => {
    if (running || stopped) return
    running = true
    while (!stopped) {
      const started = version
      try {
        await run(() => !stopped && version === started)
        delay = 1000
      } catch {
        if (version !== started) continue
        timer = setTimeout(() => { void work() }, delay)
        delay = Math.min(delay * 2, 60000)
        break
      }
      if (version === started) break
    }
    running = false
  }
  return {
    request() { version++; clearTimeout(timer); void work() },
    dispose() { stopped = true; clearTimeout(timer) },
  }
}
