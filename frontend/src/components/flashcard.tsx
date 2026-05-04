import { useState, useEffect, useRef, useCallback } from 'react'
import type { Card } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Card as UICard, CardFooter, CardHeader } from '@/components/ui/card'
import { cn, getDifficultyColor } from '@/lib/utils'
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

  // Split sentence at target word position
  const targetIndex = card.sentence.indexOf(card.target)
  const before = targetIndex > 0 ? card.sentence.substring(0, targetIndex) : ''
  const after = targetIndex >= 0 ? card.sentence.substring(targetIndex + card.target.length) : ''

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (answered) return
    const isCorrect = input.trim() === card.target
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

  const handleSuppress = async () => {
    if (suppressing) return
    setSuppressing(true)
    try {
      await suppressCard(card.card_id)
      onSuppress?.()
    } catch (err) {
      console.error('Error suppressing card:', err)
      setSuppressing(false)
    }
  }

  return (
    <UICard className="w-full max-w-xl">
      <CardHeader>
        <div className="flex items-center justify-between gap-2 mb-2">
          <div className="flex items-center gap-1.5">
          {card.guess_count === 0 ? (
            <span className="inline-block text-xs px-2 py-0.5 rounded-full bg-orange-100 dark:bg-orange-900/30 text-orange-700 dark:text-orange-400 font-medium">
              New
            </span>
          ) : (
            settings.showPercentage && card.difficulty !== null && (
              <span className={cn("text-xs px-2 py-0.5 rounded-full font-medium", getDifficultyColor(card.difficulty))}>
                {card.difficulty.toFixed(1)}
              </span>
            )
          )}
          </div>
          <div className="flex flex-wrap items-center justify-center gap-1.5 flex-1">
          {card.pos && (
            <span className="inline-block text-xs px-2 py-0.5 rounded-full bg-secondary text-secondary-foreground">
              {card.pos}
            </span>
          )}
          {card.speech_level && (
            <span className="inline-block text-xs px-2 py-0.5 rounded-full bg-secondary text-secondary-foreground">
              {card.speech_level}
            </span>
          )}
          {card.tense && (
            <span className="inline-block text-xs px-2 py-0.5 rounded-full bg-secondary text-secondary-foreground">
              {card.tense}
            </span>
          )}
          </div>
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

        <form onSubmit={handleSubmit}>
          <p className="text-2xl md:text-3xl font-semibold leading-snug text-center flex flex-wrap items-baseline justify-center gap-1">
            <span>{before}</span>
            {isAutoProgressing ? (
              // Show green answer during auto-progress for positive feedback
              <span className="text-green-600">{card.target}</span>
            ) : answered ? (
              correct ? (
                <span className="text-green-600">{card.target}</span>
              ) : (
                <span className="inline-flex flex-wrap items-baseline gap-0">
                  {input.split('').map((char, i) => (
                    <span key={i} className={char === card.target[i] ? 'text-green-600' : 'text-destructive'}>
                      {char}
                    </span>
                  ))}
                  <span className="text-muted-foreground/50 ml-1">({card.target})</span>
                </span>
              )
            ) : (
              <span className="inline-flex flex-col items-center">
                {card.hanja && (
                  <span className="text-xs text-muted-foreground/60 mb-0.5">
                    {card.hanja}
                  </span>
                )}
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
              </span>
            )}
            <span>{after}</span>
          </p>
        </form>

        <p className="text-sm text-muted-foreground text-center mt-2">
          {card.trans_word}
        </p>
        <p className="text-xs text-muted-foreground/70 text-center italic">
          {card.sentence_translation}
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
              {correct ? "Correct!" : `The answer was: ${card.target}`}
            </p>
            {card.definition && (
              <div className="text-xs text-muted-foreground space-y-1">
                <p>{card.definition}</p>
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