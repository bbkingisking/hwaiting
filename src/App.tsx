import { useState, useEffect } from 'react'
import { Flashcard } from '@/components/flashcard'
import { ThemeProvider } from '@/components/theme-provider'
import { SettingsProvider } from '@/components/settings-provider'
import { AuthProvider, useAuth } from '@/components/auth-provider'
import { AuthDialog } from '@/components/auth-dialog'
import { AppHeader } from '@/components/app-header'
import { getNextCard, submitReview, ApiError } from '@/lib/api'
import type { Card } from '@/lib/types'

function AppContent() {
  const [card, setCard] = useState<Card | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [authDialogOpen, setAuthDialogOpen] = useState(false)
  const { isAuthenticated } = useAuth()

  useEffect(() => {
    if (!isAuthenticated) {
      setAuthDialogOpen(true)
    } else {
      // Small delay to ensure token is saved to localStorage
      const timer = setTimeout(() => {
        fetchCard()
      }, 100)
      return () => clearTimeout(timer)
    }
  }, [isAuthenticated])

  const fetchCard = async () => {
    setLoading(true)
    setError(null)
    try {
      const nextCard = await getNextCard()
      setCard(nextCard)
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

  const handleReview = async (rating: number) => {
    if (!card) return

    try {
      await submitReview(card.word_id, rating)
      // Fetch next card after successful review
      await fetchCard()
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.message)
      } else {
        setError('Failed to submit review')
      }
      console.error('Error submitting review:', err)
    }
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
        ) : error ? (
          <div className="text-center">
            <p className="text-destructive mb-4">{error}</p>
            <button
              onClick={fetchCard}
              className="text-sm text-primary hover:underline"
            >
              Try again
            </button>
          </div>
        ) : card ? (
          <Flashcard
            key={card.word_id}
            card={card}
            onReview={handleReview}
          />
        ) : (
          <div className="text-center text-muted-foreground">
            <p>No cards available</p>
          </div>
        )}
      </div>
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