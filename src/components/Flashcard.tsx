import { useState, useEffect, useRef, useCallback } from 'react'
import type { Word } from '@/lib/words'
import { Button } from '@/components/ui/button'
import { Card, CardFooter, CardHeader } from '@/components/ui/card'
import { cn } from '@/lib/utils'
import { useSettings } from '@/components/settings-provider'

interface FlashcardProps {
  word: Word
  onNext: () => void
}

export function Flashcard({ word, onNext }: FlashcardProps) {
  const { settings } = useSettings()
  const [input, setInput] = useState('')
  const [answered, setAnswered] = useState(false)
  const [correct, setCorrect] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)

  const percentage = Math.round(word.correctRate * 100)
  const getRateColor = () => {
    if (percentage < settings.redThreshold) return 'text-destructive'
    if (percentage < settings.yellowThreshold) return 'text-yellow-600 dark:text-yellow-500'
    return 'text-green-600 dark:text-green-500'
  }

  const handleAdvance = useCallback(() => {
    if (answered) {
      onNext()
    }
  }, [answered, onNext])

  useEffect(() => {
    setInput('')
    setAnswered(false)
    setCorrect(false)
    inputRef.current?.focus()
  }, [word])

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (answered && (e.key === 'Enter' || e.key === ' ')) {
        e.preventDefault()
        handleAdvance()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [answered, handleAdvance])

  const idx = word.context.indexOf(word.form)
  const before = idx >= 0 ? word.context.slice(0, idx) : word.context
  const after = idx >= 0 ? word.context.slice(idx + word.form.length) : ''

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (answered) return
    const isCorrect = input.trim() === word.form
    setCorrect(isCorrect)
    setAnswered(true)
  }



  return (
    <Card className="w-full max-w-xl">
      <CardHeader className="relative">
        {/* Correct rate percentage */}
        {settings.showPercentage && (
          <div className="absolute top-4 right-4">
            <span className={cn("text-xs font-semibold", getRateColor())}>
              {percentage}%
            </span>
          </div>
        )}

        {/* Tags */}
        <div className="flex flex-wrap items-center justify-center gap-1.5 mb-2">
          {word.grammar && (
            <span className="inline-block text-xs px-2 py-0.5 rounded-full bg-secondary text-secondary-foreground">
              {word.grammar}
            </span>
          )}
          {word.politeness && (
            <span className="inline-block text-xs px-2 py-0.5 rounded-full bg-secondary text-secondary-foreground">
              {word.politeness}
            </span>
          )}
        </div>

        {/* Sentence with inline input */}
        <form onSubmit={handleSubmit}>
          <p className="text-2xl md:text-3xl font-semibold leading-snug text-center flex flex-wrap items-baseline justify-center gap-1">
            <span>{before}</span>
            {answered ? (
              correct ? (
                <span className="text-green-600">{word.form}</span>
              ) : (
                <span className="inline-flex flex-wrap items-baseline gap-0">
                  {input.split('').map((char, i) => (
                    <span key={i} className={char === word.form[i] ? 'text-green-600' : 'text-destructive'}>
                      {char}
                    </span>
                  ))}
                  <span className="text-muted-foreground/50 ml-1">({word.form})</span>
                </span>
              )
            ) : (
              <input
                ref={inputRef}
                type="text"
                value={input}
                onChange={(e) => setInput(e.target.value)}
                size={Math.max(input.length, 1)}
                className={cn(
                  'flex-none bg-transparent border-0 border-b-2 border-foreground/30',
                  'text-center text-2xl md:text-3xl font-semibold',
                  'outline-none pb-0.5',
                  'focus:border-primary transition-colors',
                )}
                autoFocus
              />
            )}
            <span>{after}</span>
          </p>
        </form>

        {/* Hint + translation */}
        <p className="text-sm text-muted-foreground text-center mt-2">
          {word.hint}
        </p>
        <p className="text-xs text-muted-foreground/70 text-center italic">
          {word.contextTranslation}
        </p>
      </CardHeader>

      <CardFooter className="flex-col gap-3">
        {!answered ? (
          <Button type="submit" onClick={handleSubmit} className="w-full">
            Check
          </Button>
        ) : (
          <>
            <p className={cn("text-sm font-medium", correct ? "text-green-600" : "text-destructive")}>
              {correct ? "Correct!" : `The answer was: ${word.form}`}
            </p>
            {word.notes.length > 0 && (
              <div className="text-xs text-muted-foreground space-y-1">
                {word.notes.map((note, i) => (
                  <p key={i}>{note}</p>
                ))}
              </div>
            )}
            <Button onClick={onNext} variant="outline" className="w-full">
              Next
            </Button>
          </>
        )}
      </CardFooter>
    </Card>
  )
}
