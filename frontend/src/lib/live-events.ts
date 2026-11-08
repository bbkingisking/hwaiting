import type { Change } from './api-generated/types.gen'

export type LiveEvent = Change & { mutation_id?: string | null }
export type ReceivedEvent = LiveEvent & { own: boolean }
const listeners = new Set<(event: ReceivedEvent) => void>()
const mutations = new Set<string>()

export function registerMutation(): string {
  // crypto.randomUUID is unavailable on plain HTTP LAN origins.
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  bytes[6] = (bytes[6] & 15) | 64
  bytes[8] = (bytes[8] & 63) | 128
  const hex = Array.from(bytes, b => b.toString(16).padStart(2, '0')).join('')
  const id = `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
  mutations.add(id)
  if (mutations.size > 1024) mutations.delete(mutations.values().next().value!)
  return id
}
export function subscribeLiveEvents(listener: (event: ReceivedEvent) => void) {
  listeners.add(listener)
  return () => { listeners.delete(listener) }
}
export function dispatchLiveEvent(event: LiveEvent) {
  const received = { ...event, own: !!event.mutation_id && mutations.has(event.mutation_id) }
  for (const listener of listeners) listener(received)
}
const names = ['card-reviewed', 'queue-changed', 'settings-changed', 'card-updated', 'state-reset', 'resync-required'] as const

// EventSource owns ordinary reconnections. A fatal HTTP error closes it;
// probe auth before retrying rather than interpreting every outage as logout.
export function connectLiveEvents(url: string, checkSession: () => Promise<boolean>) {
  let source: EventSource | undefined
  let stopped = false
  let retry: ReturnType<typeof setTimeout> | undefined
  let probing = false
  let delay = 1000
  const connect = () => {
    if (stopped) return
    clearTimeout(retry)
    source?.close()
    source = new EventSource(url, { withCredentials: true })
    for (const name of names) source.addEventListener(name, event => {
      try {
        const data = JSON.parse((event as MessageEvent).data)
        if (data.type !== name) throw new Error('Mismatched event type')
        dispatchLiveEvent(data)
      } catch {
        dispatchLiveEvent({ type: 'resync-required' })
      }
    })
    source.onopen = () => { delay = 1000 }
    source.onerror = () => {
      if (!probing) void probe()
    }
  }
  const probe = async () => {
    probing = true
    try {
      const authenticated = await checkSession()
      if (stopped) return
      if (!authenticated) { source?.close(); stopped = true; return }
    } catch { /* Network failure: retain the session and retry. */ }
    finally { probing = false }
    if (!stopped && source?.readyState === EventSource.CLOSED) {
      retry = setTimeout(connect, delay)
      delay = Math.min(delay * 2, 60000)
    }
  }
  // Reopen disconnected streams on return, without refetching on every visit.
  // A new subscription resynchronises any changes missed while disconnected.
  const resume = () => { if (document.visibilityState === 'visible' && source?.readyState !== EventSource.OPEN) connect() }
  document.addEventListener('visibilitychange', resume)
  window.addEventListener('online', resume)
  connect()
  return () => {
    stopped = true
    source?.close()
    clearTimeout(retry)
    document.removeEventListener('visibilitychange', resume)
    window.removeEventListener('online', resume)
  }
}
