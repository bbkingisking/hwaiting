import { useState, useEffect, useRef } from 'react'
import { Flashcard } from '@/components/flashcard'
import { ThemeProvider } from '@/components/theme-provider'
import { SettingsProvider } from '@/components/settings-provider'

import { AuthProvider, useAuth } from '@/components/auth-provider'
import { AuthDialog } from '@/components/auth-dialog'
import { AppHeader } from '@/components/app-header'
import { LanguageSelector } from '@/components/language-selector'
import { StatusIndicator } from '@/components/status-indicator'
import { getNextCard, submitReview, getUserProfile, getStats, ApiError } from '@/lib/api'
import type { Card } from '@/lib/types'

// Tracks a background fetch for the next card. The same slot moves
// through three states: in-flight (result === null, promise pending),
// resolved (result !== null), or aborted (controller.signal.aborted).
type PrefetchSlot = {
  abort: AbortController
  promise: Promise<Card | null>
  result: Card | null
}

function AppContent() {
  const [card, setCard] = useState<Card | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [noCards, setNoCards] = useState(false)
  const [nextDueAt, setNextDueAt] = useState<string | null>(null)
  const [authDialogOpen, setAuthDialogOpen] = useState(false)
  const [needsLanguage, setNeedsLanguage] = useState<boolean | null>(null)
  const [statsKey, setStatsKey] = useState(0)
  const { isAuthenticated } = useAuth()

  // The next card prefetched in the background while the user works on
  // the current one. Held in a ref so async flows can read/write it
  // synchronously without triggering re-renders.
  const prefetchRef = useRef<PrefetchSlot | null>(null)

  useEffect(() => {
    if (!isAuthenticated) {
      setAuthDialogOpen(true)
      setNeedsLanguage(null)
    } else {
      // Check if user has a target language set
      checkUserLanguage()
    }
  }, [isAuthenticated])

  const checkUserLanguage = async () => {
    try {
      const profile = await getUserProfile()
      if (profile.target_language === null) {
        setNeedsLanguage(true)
      } else {
        setNeedsLanguage(false)
        loadCardCold()
      }
    } catch (err) {
      console.error('Error checking user language:', err)
      // If we can't check, assume they need to set it
      setNeedsLanguage(true)
    }
  }

  const handleLanguageSelected = () => {
    setNeedsLanguage(false)
    loadCardCold()
  }

  const formatTimeUntil = (isoTimestamp: string): string | null => {
    const now = new Date()
    const due = new Date(isoTimestamp)
    const diffMs = due.getTime() - now.getTime()
    if (diffMs <= 0) return null
    const diffMinutes = Math.floor(diffMs / 60000)
    const hours = Math.floor(diffMinutes / 60)
    const minutes = diffMinutes % 60
    if (hours > 0) return `${hours}h ${minutes}m`
    return `${minutes}m`
  }

  // Cancel any in-flight prefetch and clear the cached prefetched card.
  const cancelPrefetch = () => {
    if (prefetchRef.current) {
      prefetchRef.current.abort.abort()
      prefetchRef.current = null
    }
  }

  // Kick off a background fetch for the card after `currentWordId`.
  // If `waitFor` is provided, the actual network request is delayed
  // until that promise settles - used to avoid racing an in-flight
  // optimistic review submission (otherwise the DB read can return
  // the just-reviewed card again before its due_date is updated).
  const startPrefetch = (
    currentWordId: number,
    waitFor?: Promise<unknown>,
  ): PrefetchSlot => {
    cancelPrefetch()

    const controller = new AbortController()
    const slot: PrefetchSlot = {
      abort: controller,
      result: null,
      // Filled in below; declared here so the closure can reference `slot`.
      promise: Promise.resolve(null),
    }

    slot.promise = (async () => {
      try {
        if (waitFor) {
          await waitFor
          // A newer prefetch may have superseded this one while we
          // were waiting; bail out if so.
          if (prefetchRef.current !== slot) return null
        }
        const next = await getNextCard({
          excludeWordId: currentWordId,
          signal: controller.signal,
        })
        // Only commit if this prefetch wasn't superseded.
        if (prefetchRef.current === slot) {
          slot.result = next
        }
        return next
      } catch (err) {
        if (err instanceof Error && err.name !== 'AbortError') {
          console.debug('Prefetch failed:', err)
        }
        return null
      }
    })()

    prefetchRef.current = slot
    return slot
  }

  // Cold load: blocks the UI with a loading state. Used for the very
  // first card, after errors, and after suppression.
  const loadCardCold = async () => {
    cancelPrefetch()
    setLoading(true)
    setError(null)
    setNoCards(false)
    try {
      const next = await getNextCard()
      setCard(next)
      setNoCards(false)
      startPrefetch(next.word_id)
    } catch (err) {
      if (err instanceof ApiError && err.status === 404) {
        // No cards available — show friendly message with next due time
        setCard(null)
        setNoCards(true)
        try {
          const stats = await getStats()
          setNextDueAt(stats.next_due_at)
        } catch {
          setNextDueAt(null)
        }
      } else if (err instanceof ApiError) {
        setError(err.message)
      } else {
        setError('Failed to load card')
      }
      console.error('Error fetching card:', err)
    } finally {
      setLoading(false)
    }
  }

  // Advance to the next card. Uses the prefetched card if ready;
  // otherwise awaits the in-flight prefetch (no extra request); falls
  // back to a cold load only if no prefetch was ever started.
  // `waitForBeforePrefetch` defers the *next* prefetch (for N+2) until
  // the given promise settles, so we don't read stale DB state while
  // the previous review is still being persisted.
  const advanceToNextCard = async (waitForBeforePrefetch?: Promise<unknown>) => {
    setError(null)
    setNoCards(false)

    const slot = prefetchRef.current

    // Fast path: prefetch already resolved.
    if (slot?.result) {
      const next = slot.result
      prefetchRef.current = null
      setCard(next)
      startPrefetch(next.word_id, waitForBeforePrefetch)
      return
    }

    // Prefetch in flight: await it rather than firing a duplicate.
    if (slot) {
      setLoading(true)
      try {
        const next = await slot.promise
        if (next) {
          prefetchRef.current = null
          setCard(next)
          startPrefetch(next.word_id, waitForBeforePrefetch)
          return
        }
      } finally {
        setLoading(false)
      }
    }

    // Cold path: no prefetch available, do a regular fetch.
    await loadCardCold()
  }

  // Optimistic review: advance immediately, submit in the background.
  // On submission failure we surface an error banner but don't roll
  // back the UI - the user has already moved on.
  const handleReview = (rating: number) => {
    if (!card) return

    // Fire submission. The returned promise is used to defer the
    // *next* prefetch (for N+2) until the DB has the updated due_date
    // for this card; otherwise the prefetch can return the same card
    // again because it still looks "due now" in the DB.
    const submitPromise = submitReview(card.word_id, rating).catch((err) => {
      if (err instanceof ApiError) {
        setError(`Failed to submit review: ${err.message}`)
      } else {
        setError('Failed to submit review')
      }
      console.error('Error submitting review:', err)
    })

    // Bump stats counter immediately for snappy UX. If the submit
    // ultimately fails the counter will be slightly off until the
    // next refresh - acceptable trade-off.
    setStatsKey((prev) => prev + 1)

    // Move to next card right away. The prefetch for N+2 waits on
    // submitPromise so it sees a consistent DB state.
    void advanceToNextCard(submitPromise)
  }

  const handleSuppress = async () => {
    // Suppression invalidates the prefetched card (it might be the
    // one we just suppressed, or its ordering might have shifted).
    await loadCardCold()
    setStatsKey((prev) => prev + 1)
  }

  // Show language selector if authenticated and needs to select language
  if (isAuthenticated && needsLanguage === true) {
    return (
      <>
        <AppHeader />
        <LanguageSelector onLanguageSelected={handleLanguageSelected} />
      </>
    )
  }

  return (
    <>
      <AuthDialog open={authDialogOpen} onOpenChange={setAuthDialogOpen} />
      <AppHeader />
      <div className="min-h-screen flex flex-col items-center justify-center p-6">
        {!isAuthenticated ? (
          <div className="text-center text-muted-foreground">
            <p>Please log in to continue</p>
          </div>
        ) : needsLanguage === null ? (
          <div className="text-center text-muted-foreground">
            <p>Loading...</p>
          </div>
        ) : loading ? (
          <div className="text-center text-muted-foreground">
            <p>Loading card...</p>
          </div>
        ) : noCards && !card ? (
          <div className="text-center text-muted-foreground">
            <p className="mb-2">No cards to review right now</p>
            {nextDueAt && (() => {
              const formatted = formatTimeUntil(nextDueAt)
              return formatted ? <p className="text-sm">Next in {formatted}</p> : null
            })()}
          </div>
        ) : error && !card ? (
          <div className="text-center">
            <p className="text-destructive mb-4">{error}</p>
            <button
              onClick={loadCardCold}
              className="text-sm text-primary hover:underline"
            >
              Try again
            </button>
          </div>
        ) : card ? (
          <>
            {error && (
              <p className="text-destructive text-sm mb-4">{error}</p>
            )}
            <Flashcard
              key={card.word_id}
              card={card}
              onReview={handleReview}
              onSuppress={handleSuppress}
            />
          </>
        ) : (
          <div className="text-center text-muted-foreground">
            <p>No cards available</p>
          </div>
        )}
      </div>
      {isAuthenticated && needsLanguage === false && <StatusIndicator key={statsKey} />}
    </>
  )
}

function App() {
  return (
    <ThemeProvider>
      <AuthProvider>
        <SettingsProvider>
          <AppContent />
        </SettingsProvider>
      </AuthProvider>
    </ThemeProvider>
  )
}

export default App
