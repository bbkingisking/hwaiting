import { useState, useEffect, useRef, useCallback } from 'react'
import type { Card } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Card as UICard, CardFooter, CardHeader } from '@/components/ui/card'
import { cn, splitSentence } from '@/lib/utils'
import { KEYS } from '@/lib/constants'
import { useSettings } from '@/components/settings-provider'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { MoreVertical } from 'lucide-react'
import { suppressCard } from '@/lib/api'

interface FlashcardProps {
  card: Card
  onReview: (rating: number) => void
  onSuppress?: () => void
}

export function Flashcard({ card, onReview, onSuppress }: FlashcardProps) {
  const [input, setInput] = useState('')
  const [answered, setAnswered] = useState(false)
  const [correct, setCorrect] = useState(false)
  const [suppressing, setSuppressing] = useState(false)
  const [isAutoProgressing, setIsAutoProgressing] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  const { settings } = useSettings()
  const hasAutoProgressedRef = useRef(false)

  const handleAdvance = useCallback(() => {
    if (answered) {
      // Submit review: 1 = Again (wrong), 3 = Good (correct)
      const rating = correct ? 3 : 1
      onReview(rating)
    }
  }, [answered, correct, onReview])

  useEffect(() => {
    setInput('')
    setAnswered(false)
    setCorrect(false)
    setIsAutoProgressing(false)
    hasAutoProgressedRef.current = false
    inputRef.current?.focus()
  }, [card])

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (answered && (e.key === KEYS.ENTER || e.key === KEYS.SPACE)) {
        e.preventDefault()
        handleAdvance()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [answered, handleAdvance])

  const { before, after } = splitSentence(card.context, card.form)

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (answered) return
    const isCorrect = input.trim() === card.form
    setCorrect(isCorrect)
    
    // Auto-progress if correct and setting is enabled
    if (isCorrect && settings.autoProgressOnCorrect && !hasAutoProgressedRef.current) {
      hasAutoProgressedRef.current = true
      setIsAutoProgressing(true)
      // Use configurable delay before progressing
      setTimeout(() => {
        onReview(3) // 3 = Good (correct)
      }, settings.autoProgressDelay)
    } else {
      // Only set answered state if not auto-progressing
      setAnswered(true)
    }
  }

  const getPercentageColor = () => {
    const rate = card.correct_rate
    if (rate >= settings.yellowThreshold) return 'text-green-600 dark:text-green-500'
    if (rate >= settings.redThreshold) return 'text-yellow-600 dark:text-yellow-500'
    return 'text-destructive'
  }

  const handleSuppress = async () => {
    if (suppressing) return
    setSuppressing(true)
    try {
      await suppressCard(card.word_id)
      onSuppress?.()
    } catch (err) {
      console.error('Error suppressing card:', err)
      setSuppressing(false)
    }
  }

  return (
    <UICard className="w-full max-w-xl">
      <CardHeader className="relative">
        <div className="absolute top-4 right-4 flex items-center gap-2">
          {settings.showPercentage && card.guess_count > 0 && (
            <span className={cn("text-sm font-medium", getPercentageColor())}>
              {Math.round(card.correct_rate)}%
            </span>
          )}
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 text-muted-foreground hover:text-foreground"
                  disabled={suppressing}
                >
                  <MoreVertical className="h-4 w-4" />
                </Button>
              }
            />
            <DropdownMenuContent align="end" className="min-w-50">
              <DropdownMenuItem
                onClick={handleSuppress}
                disabled={suppressing}
                variant="destructive"
              >
                Don't show this card again
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        <div className="flex flex-wrap items-center justify-center gap-1.5 mb-2">
          {card.grammar && (
            <span className="inline-block text-xs px-2 py-0.5 rounded-full bg-secondary text-secondary-foreground">
              {card.grammar}
            </span>
          )}
          {card.politeness && (
            <span className="inline-block text-xs px-2 py-0.5 rounded-full bg-secondary text-secondary-foreground">
              {card.politeness}
            </span>
          )}
        </div>

        <form onSubmit={handleSubmit}>
          <p className="text-2xl md:text-3xl font-semibold leading-snug text-center flex flex-wrap items-baseline justify-center gap-1">
            <span>{before}</span>
            {isAutoProgressing ? (
              // Show green answer during auto-progress for positive feedback
              <span className="text-green-600">{card.form}</span>
            ) : answered ? (
              correct ? (
                <span className="text-green-600">{card.form}</span>
              ) : (
                <span className="inline-flex flex-wrap items-baseline gap-0">
                  {input.split('').map((char, i) => (
                    <span key={i} className={char === card.form[i] ? 'text-green-600' : 'text-destructive'}>
                      {char}
                    </span>
                  ))}
                  <span className="text-muted-foreground/50 ml-1">({card.form})</span>
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
                  'focus:border-primary',
                  !settings.autoProgressOnCorrect && 'transition-colors',
                )}
                autoFocus
              />
            )}
            <span>{after}</span>
          </p>
        </form>

        <p className="text-sm text-muted-foreground text-center mt-2">
          {card.hint}
        </p>
        <p className="text-xs text-muted-foreground/70 text-center italic">
          {card.context_translation}
        </p>
      </CardHeader>

      <CardFooter className={cn("flex-col gap-3", isAutoProgressing && "invisible")}>
        {!answered && !isAutoProgressing ? (
          <Button type="submit" onClick={handleSubmit} className="w-full">
            Check
          </Button>
        ) : (
          <>
            <p className={cn("text-sm font-medium", correct ? "text-green-600" : "text-destructive")}>
              {correct ? "Correct!" : `The answer was: ${card.form}`}
            </p>
            {card.notes.length > 0 && (
              <div className="text-xs text-muted-foreground space-y-1">
                {card.notes.map((note, i) => (
                  <p key={i}>{note}</p>
                ))}
              </div>
            )}
            <Button onClick={handleAdvance} variant="outline" className="w-full">
              Next
            </Button>
          </>
        )}
      </CardFooter>
    </UICard>
  )
}