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

// Tracks a background fetch for the next card.
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
  // Fires immediately — no waitFor. The `exclude` parameter prevents
  // the API from returning the current card.
  const startPrefetch = (currentCardId: number): PrefetchSlot => {
    cancelPrefetch()

    const controller = new AbortController()
    const slot: PrefetchSlot = {
      abort: controller,
      result: null,
      promise: Promise.resolve(null),
    }

    slot.promise = (async () => {
      try {
        const envelope = await getNextCard({
          excludeCardId: currentCardId,
          signal: controller.signal,
        })
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

  // Cold load: shows a loading state. Used for initial load, after
  // errors, after suppress, and when cards become available again.
  const loadCardCold = async () => {
    cancelPrefetch()
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
  // otherwise fetches fresh. Never sets loading=true during a
  // card-to-card transition — the old card stays visible until
  // the new one arrives, so there's no flash.
  const advanceToNextCard = async () => {
    setError(null)
    setNoCards(false)

    const slot = prefetchRef.current

    // Fast path: prefetch already resolved with a card.
    if (slot?.result?.card) {
      const nextCard = slot.result.card
      prefetchRef.current = null
      setCard(nextCard)
      setNoCards(false)
      startPrefetch(nextCard.card_id)
      return
    }

    // Any other case (in-flight, null result, no prefetch): fetch
    // fresh. Old card stays visible during the fetch.
    prefetchRef.current = null
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
        setError('Failed to load next card')
      }
      console.error('Error fetching next card:', err)
    }
  }

  const handleReview = async (rating: number) => {
    if (!card) return

    setStatsKey((prev) => prev + 1)

    try {
      await submitReview(card.card_id, rating)
    } catch (err) {
      if (err instanceof ApiError) {
        setError(`Failed to submit review: ${err.message}`)
      } else {
        setError('Failed to submit review')
      }
      console.error('Error submitting review:', err)
    }

    await advanceToNextCard()
  }

  const handleSuppress = async () => {
    await loadCardCold()
    setStatsKey((prev) => prev + 1)
  }

  const handleCardUpdated = (updates: Partial<Card>) => {
    const filtered = Object.fromEntries(
      Object.entries(updates).filter(([, v]) => v !== undefined)
    ) as Partial<Card>
    setCard(prev => prev ? { ...prev, ...filtered } : prev)
  }

  const handleCardUpdated = (updates: Partial<Card>) => {
    const filtered = Object.fromEntries(
      Object.entries(updates).filter(([, v]) => v !== undefined)
    ) as Partial<Card>
    setCard(prev => prev ? { ...prev, ...filtered } : prev)
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
              onCardUpdated={handleCardUpdated}
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
