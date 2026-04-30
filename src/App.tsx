import { useState, useEffect } from 'react'
import { Flashcard } from '@/components/flashcard'
import { ThemeProvider } from '@/components/theme-provider'
import { SettingsProvider } from '@/components/settings-provider'

import { AuthProvider, useAuth } from '@/components/auth-provider'
import { AuthDialog } from '@/components/auth-dialog'
import { AppHeader } from '@/components/app-header'
import { LanguageSelector } from '@/components/language-selector'
import { StatusIndicator } from '@/components/status-indicator'
import { getNextCard, submitReview, getUserProfile, ApiError } from '@/lib/api'
import type { Card } from '@/lib/types'

function AppContent() {
  const [card, setCard] = useState<Card | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [authDialogOpen, setAuthDialogOpen] = useState(false)
  const [needsLanguage, setNeedsLanguage] = useState<boolean | null>(null)
  const [statsKey, setStatsKey] = useState(0)
  const { isAuthenticated } = useAuth()

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
        // Small delay to ensure everything is set up
        const timer = setTimeout(() => {
          fetchCard()
        }, 100)
        return () => clearTimeout(timer)
      }
    } catch (err) {
      console.error('Error checking user language:', err)
      // If we can't check, assume they need to set it
      setNeedsLanguage(true)
    }
  }

  const handleLanguageSelected = () => {
    setNeedsLanguage(false)
    fetchCard()
  }

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
      // Trigger stats refresh
      setStatsKey(prev => prev + 1)
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.message)
      } else {
        setError('Failed to submit review')
      }
      console.error('Error submitting review:', err)
    }
  }

  const handleSuppress = async () => {
    // Fetch next card after suppressing
    await fetchCard()
    // Trigger stats refresh
    setStatsKey(prev => prev + 1)
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
            onSuppress={handleSuppress}
          />
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