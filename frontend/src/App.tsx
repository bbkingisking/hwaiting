import { useState, useEffect } from 'react'
import { DotLottieReact } from '@lottiefiles/dotlottie-react'
import { Flashcard } from '@/components/flashcard'
import { ThemeProvider } from '@/components/theme-provider'
import { SettingsProvider } from '@/components/settings-provider'

import { AuthProvider, useAuth } from '@/components/auth-provider'
import { AuthDialog } from '@/components/auth-dialog'
import { AppHeader } from '@/components/app-header'
import { StatusIndicator } from '@/components/status-indicator'
import { getNextCard, submitReview, ApiError } from '@/lib/api'
import type { Card } from '@/lib/types'
import { formatTimeUntil } from '@/lib/utils'

function AppContent() {
  const [card, setCard] = useState<Card | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [noCards, setNoCards] = useState(false)
  const [nextDueAt, setNextDueAt] = useState<string | null>(null)
  const [authDialogOpen, setAuthDialogOpen] = useState(false)
  const [statsKey, setStatsKey] = useState(0)

  const { isAuthenticated } = useAuth()

  useEffect(() => {
    if (!isAuthenticated) {
      setAuthDialogOpen(true)
    } else {
      loadCard()
    }
  }, [isAuthenticated])

  // Fetch the next card from the API. Shows a loading indicator only
  // when there is no current card to display (initial load, after
  // suppress). During card-to-card transitions the old card stays
  // visible until the new one arrives — no flash.
  const loadCard = async (showLoading = true) => {
    if (showLoading) setLoading(true)
    setError(null)
    setNoCards(false)
    try {
      const envelope = await getNextCard()
      if (envelope.card) {
        setCard(envelope.card)
        setNoCards(false)
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
      if (showLoading) setLoading(false)
    }
  }

  const handleReview = async (rating: number) => {
    if (!card) return
    const reviewedId = card.card_id

    // Bump stats counter immediately for snappy UX.
    setStatsKey((prev) => prev + 1)

    try {
      await submitReview(reviewedId, rating)
    } catch (err) {
      if (err instanceof ApiError) {
        setError(`Failed to submit review: ${err.message}`)
      } else {
        setError('Failed to submit review')
      }
      console.error('Error submitting review:', err)
    }

    // Fetch next card without a loading state — the old card stays
    // visible until the new one arrives, so there's no flash.
    try {
      const envelope = await getNextCard({ excludeCardId: reviewedId })
      if (envelope.card) {
        setCard(envelope.card)
        setNoCards(false)
        setError(null)
      } else {
        setCard(null)
        setNoCards(true)
        setNextDueAt(envelope.next_due_at)
        setError(null)
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

  const handleSuppress = async () => {
    await loadCard()
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
              onClick={() => loadCard()}
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
      {isAuthenticated && <StatusIndicator key={statsKey} onCardsAvailable={() => loadCard()} />}
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
