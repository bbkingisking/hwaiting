import { useState, useEffect, useRef } from 'react'
import type { Word } from '@/lib/words'
import { Button } from '@/components/ui/button'
import { Card, CardFooter, CardHeader } from '@/components/ui/card'
import { cn } from '@/lib/utils'

interface FlashcardProps {
  word: Word
  onNext: () => void
}

export function Flashcard({ word, onNext }: FlashcardProps) {
  const [input, setInput] = useState('')
  const [answered, setAnswered] = useState(false)
  const [correct, setCorrect] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    setInput('')
    setAnswered(false)
    setCorrect(false)
    inputRef.current?.focus()
  }, [word])

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

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === 'Enter' && answered) {
      e.preventDefault()
      onNext()
    }
  }

  return (
    <Card className="w-full max-w-xl">
      <CardHeader>
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
                onKeyDown={handleKeyDown}
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
