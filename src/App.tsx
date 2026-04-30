import { useState, useEffect } from 'react'
import { getRandomWord } from '@/lib/words'
import { Flashcard } from '@/components/flashcard'
import { ThemeProvider } from '@/components/theme-provider'
import { SettingsProvider } from '@/components/settings-provider'
import { AuthProvider, useAuth } from '@/components/auth-provider'
import { AuthDialog } from '@/components/auth-dialog'
import { AppHeader } from '@/components/app-header'

function AppContent() {
  const [word, setWord] = useState(getRandomWord)
  const [authDialogOpen, setAuthDialogOpen] = useState(false)
  const { isAuthenticated } = useAuth()

  useEffect(() => {
    if (!isAuthenticated) {
      setAuthDialogOpen(true)
    }
  }, [isAuthenticated])

  return (
    <>
      <AuthDialog open={authDialogOpen} onOpenChange={setAuthDialogOpen} />
      <AppHeader />
      <div className="min-h-screen flex flex-col items-center justify-center p-6">
        {isAuthenticated ? (
          <Flashcard key={word.context} word={word} onNext={() => setWord(getRandomWord())} />
        ) : (
          <div className="text-center text-muted-foreground">
            <p>Please log in to continue</p>
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