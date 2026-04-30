import { useState } from 'react'
import { getRandomWord } from '@/lib/words'
import { Flashcard } from '@/components/flashcard'
import { ThemeProvider } from '@/components/theme-provider'
import { SettingsProvider } from '@/components/settings-provider'
import { AppHeader } from '@/components/app-header'

function App() {
  const [word, setWord] = useState(getRandomWord)

  return (
    <ThemeProvider>
      <SettingsProvider>
        <AppHeader />
        <div className="min-h-screen flex flex-col items-center justify-center p-6">
          <Flashcard key={word.context} word={word} onNext={() => setWord(getRandomWord())} />
        </div>
      </SettingsProvider>
    </ThemeProvider>
  )
}

export default App
