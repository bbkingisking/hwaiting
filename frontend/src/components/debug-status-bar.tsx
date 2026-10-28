import { useSettings } from '@/components/settings-provider'

// Optional debug aid, toggled in Settings -> Appearance -> Status bar ->
// Debug info (device-local, see DEFAULT_SETTINGS.DEBUG_STATUS_BAR). Shows the build,
// card ID, and last answer-check round trip, in ms - the one network call left in
// the review loop that isn't hidden behind a prefetch (see App.tsx's
// handleCheck and the aria-busy comment on <main>).
//
// aria-hidden: this is diagnostic information, not part of the review
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

  if (!settings.debugStatusBar) return null

  return (
    <div
      aria-hidden
      className="fixed bottom-4 left-4 text-xs text-muted-foreground/50 font-mono select-none"
    >
      build hash: {import.meta.env.VITE_BUILD_HASH} · last check: {lastCheckMs !== null ? `${Math.round(lastCheckMs)}ms` : '—'} · current card: {cardId ?? '—'}
    </div>
  )
}
