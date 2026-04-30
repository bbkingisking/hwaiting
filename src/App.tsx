import { useState, useEffect } from 'react'
import { Button } from '@/components/ui/button'

const greetings = [
  { lang: 'English', text: 'Hello World' },
  { lang: 'Korean', text: '안녕 세계' },
  { lang: 'Japanese', text: 'こんにちは世界' },
  { lang: 'Chinese', text: '你好世界' },
  { lang: 'Spanish', text: 'Hola Mundo' },
  { lang: 'French', text: 'Bonjour le Monde' },
  { lang: 'German', text: 'Hallo Welt' },
  { lang: 'Italian', text: 'Ciao Mondo' },
  { lang: 'Portuguese', text: 'Olá Mundo' },
  { lang: 'Russian', text: 'Привет Мир' },
  { lang: 'Arabic', text: 'مرحبا بالعالم' },
  { lang: 'Hindi', text: 'नमस्ते दुनिया' },
  { lang: 'Thai', text: 'สวัสดีชาวโลก' },
  { lang: 'Vietnamese', text: 'Xin chào Thế giới' },
  { lang: 'Turkish', text: 'Merhaba Dünya' },
]

function App() {
  const [currentIndex, setCurrentIndex] = useState(0)

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.code === 'Space') {
        e.preventDefault()
        setCurrentIndex((prev) => (prev + 1) % greetings.length)
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [])

  const current = greetings[currentIndex]

  return (
    <div className="min-h-screen flex flex-col items-center justify-center bg-gradient-to-br from-slate-900 via-purple-900 to-slate-900">
      <div className="text-center space-y-8">
        <h1 className="text-6xl md:text-8xl font-bold text-white tracking-tight transition-all duration-300 ease-in-out">
          {current.text}
        </h1>
        <p className="text-xl text-slate-300">
          {current.lang}
        </p>
        <div className="flex items-center justify-center gap-4">
          <Button
            variant="outline"
            size="lg"
            onClick={() => setCurrentIndex((prev) => (prev + 1) % greetings.length)}
            className="text-lg"
          >
            Next Language
          </Button>
        </div>
        <p className="text-sm text-slate-400 mt-8">
          Press <kbd className="px-2 py-1 bg-slate-800 rounded text-slate-200">Space</kbd> to cycle through languages
        </p>
      </div>
    </div>
  )
}

export default App
