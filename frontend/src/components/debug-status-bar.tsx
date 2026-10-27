import { useSettings } from '@/components/settings-provider'
import { useAuth } from '@/components/auth-provider'

// Admin-only debug aid, toggled in Settings -> Admin stuff -> Debug status
// bar (device-local, see DEFAULT_SETTINGS.DEBUG_STATUS_BAR). Shows the build,
// card ID, and last answer-check round trip, in ms - the one network call left in
// the review loop that isn't hidden behind a prefetch (see App.tsx's
// handleCheck and the aria-busy comment on <main>).
//
// aria-hidden: this is developer/admin chrome, not part of the review
// card's own accessibility contract (see CLAUDE.md) - nothing here should
// show up in a read of the page.
export function DebugStatusBar({
  lastCheckMs,
  cardId,
}: {
  lastCheckMs: number | null
  cardId: number | null
}) {
  const { settings } = useSettings()
  const { isAdmin } = useAuth()

  if (!isAdmin || !settings.debugStatusBar) return null

  return (
    <div
      aria-hidden
      className="fixed bottom-4 left-4 text-xs text-muted-foreground/50 font-mono select-none"
    >
      card: {cardId ?? '—'} · check: {lastCheckMs !== null ? `${Math.round(lastCheckMs)}ms` : '—'} · build: {import.meta.env.VITE_BUILD_HASH}
    </div>
  )
}
