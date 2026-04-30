import { useState } from 'react'
import { getRandomWord } from '@/lib/words'
import { Flashcard } from '@/components/Flashcard'
import { ThemeProvider } from '@/components/theme-provider'
import { ModeToggle } from '@/components/mode-toggle'

function App() {
  const [word, setWord] = useState(getRandomWord)

  return (
    <ThemeProvider>
      <div className="fixed top-4 right-4 z-50">
        <ModeToggle />
      </div>
      <div className="min-h-screen flex flex-col items-center justify-center p-6">
        <Flashcard key={word.context} word={word} onNext={() => setWord(getRandomWord())} />
      </div>
    </ThemeProvider>
  )
}

export default App
