import { useState, useEffect, useRef } from 'react'
import { DotLottieReact } from '@lottiefiles/dotlottie-react'
import { Flashcard } from '@/components/flashcard'
import { ThemeProvider } from '@/components/theme-provider'
import { SettingsProvider } from '@/components/settings-provider'

import { AuthProvider, useAuth } from '@/components/auth-provider'
import { AuthDialog } from '@/components/auth-dialog'
import { AppHeader } from '@/components/app-header'
import { StatusIndicator } from '@/components/status-indicator'
import { getNextCard, submitReview, ApiError } from '@/lib/api'
import type { NextCardEnvelope } from '@/lib/api'
import type { Card } from '@/lib/types'
import { formatTimeUntil } from '@/lib/utils'

// Tracks a background fetch for the next card. The same slot moves
// through three states: in-flight (result === null, promise pending),
// resolved (result !== null), or aborted (controller.signal.aborted).
type PrefetchSlot = {
  abort: AbortController
  promise: Promise<NextCardEnvelope | null>
  result: NextCardEnvelope | null
}

function AppContent() {
  const [card, setCard] = useState<Card | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [noCards, setNoCards] = useState(false)
  const [nextDueAt, setNextDueAt] = useState<string | null>(null)
  const [authDialogOpen, setAuthDialogOpen] = useState(false)
  const [statsKey, setStatsKey] = useState(0)

  const { isAuthenticated } = useAuth()

  // The next card prefetched in the background while the user works on
  // the current one. Held in a ref so async flows can read/write it
  // synchronously without triggering re-renders.
  const prefetchRef = useRef<PrefetchSlot | null>(null)

  useEffect(() => {
    if (!isAuthenticated) {
      setAuthDialogOpen(true)
    } else {
      loadCardCold()
    }
  }, [isAuthenticated])

  // Cancel any in-flight prefetch and clear the cached prefetched card.
  const cancelPrefetch = () => {
    if (prefetchRef.current) {
      prefetchRef.current.abort.abort()
      prefetchRef.current = null
    }
  }

  // Kick off a background fetch for the card after `currentCardId`.
  // If `waitFor` is provided, the actual network request is delayed
  // until that promise settles - used to avoid racing an in-flight
  // optimistic review submission (otherwise the DB read can return
  // the just-reviewed card again before its due_date is updated).
  const startPrefetch = (
    currentCardId: number,
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
        const envelope = await getNextCard({
          excludeCardId: currentCardId,
          signal: controller.signal,
        })
        // Only commit if this prefetch wasn't superseded.
        if (prefetchRef.current === slot) {
          slot.result = envelope
        }
        return envelope
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
  // `waitFor` defers the fetch until a pending review submission has
  // been persisted, so the DB doesn't return the just-reviewed card.
  const loadCardCold = async (waitFor?: Promise<unknown>) => {
    cancelPrefetch()
    if (waitFor) await waitFor
    setLoading(true)
    setError(null)
    setNoCards(false)
    try {
      const envelope = await getNextCard()
      if (envelope.card) {
        setCard(envelope.card)
        setNoCards(false)
        startPrefetch(envelope.card.card_id)
      } else {
        setCard(null)
        setNoCards(true)
        setNextDueAt(envelope.next_due_at)
      }
    } catch (err) {
      if (err instanceof ApiError) {
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
  //
  // IMPORTANT: a prefetched result with card === null is NOT trusted.
  // "No cards" from a prefetch only means "no cards were due at the
  // time the prefetch fired" — which can be arbitrarily stale if the
  // user was idle (alt-tabbed, slow to review, etc.). Cards may have
  // become due in the meantime. In that case we always fall through
  // to loadCardCold for a fresh fetch. A prefetched *card* is fine
  // to use optimistically; a prefetched "no cards" is not.
  const advanceToNextCard = async (waitForBeforePrefetch?: Promise<unknown>) => {
    setError(null)
    setNoCards(false)

    const slot = prefetchRef.current

    // Fast path: prefetch already resolved with a concrete card.
    if (slot?.result?.card) {
      const nextCard = slot.result.card
      prefetchRef.current = null
      setCard(nextCard)
      setNoCards(false)
      startPrefetch(nextCard.card_id, waitForBeforePrefetch)
      return
    }

    // Prefetch in flight: await it rather than firing a duplicate.
    if (slot) {
      setLoading(true)
      try {
        const envelope = await slot.promise
        const nextCard = envelope?.card
        if (nextCard) {
          prefetchRef.current = null
          setCard(nextCard)
          setNoCards(false)
          startPrefetch(nextCard.card_id, waitForBeforePrefetch)
          return
        }
        // Prefetch returned null or card === null — can't trust it
        // (may be stale). Fall through to cold load.
      } finally {
        setLoading(false)
      }
    }

    // Cold path: no prefetch available, or prefetch had no card.
    // Do a fresh fetch. Wait for the review submission so the DB
    // reflects the updated due_date.
    await loadCardCold(waitForBeforePrefetch)
  }

  // Optimistic review: advance immediately, submit in the background.
  // On submission failure we surface an error banner but don't roll
  // back the UI - the user has already moved on.
  const handleReview = (rating: number) => {
    if (!card) return

    // Fire submission. The raw promise (without .catch) is passed to
    // advanceToNextCard so that the prefetch for N+2 actually waits
    // for the submission to complete before reading the DB - otherwise
    // the prefetch can return stale data (the just-reviewed card still
    // looks "due") or a failed submission leaves the card due while
    // the UI has already moved on, causing a stuck "No cards" state.
    const submitPromise = submitReview(card.card_id, rating)
    submitPromise.catch((err) => {
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

  return (
    <>
      <AuthDialog open={authDialogOpen} onOpenChange={setAuthDialogOpen} />
      <AppHeader />
      <div className="min-h-screen flex flex-col items-center justify-center p-6">
        {!isAuthenticated ? (
          <div className="text-center text-muted-foreground">
            <p>Please log in to continue</p>
          </div>
        ) : loading ? (
          <div className="text-center text-muted-foreground">
            <p>Loading card...</p>
          </div>
        ) : noCards && !card ? (
          <div className="text-center text-muted-foreground">
            <div className="w-64 mx-auto mb-4">
              <DotLottieReact
                src="/Taegeukgi.json"
                loop
                autoplay
              />
            </div>
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
              onClick={() => loadCardCold()}
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
              key={card.card_id}
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
      {isAuthenticated && <StatusIndicator key={statsKey} onCardsAvailable={loadCardCold} />}
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
