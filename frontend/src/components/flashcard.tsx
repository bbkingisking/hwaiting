import { useState, useEffect, useRef, useCallback } from 'react'
import type { Card, HanjaHint } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Card as UICard, CardFooter, CardHeader } from '@/components/ui/card'
import { cn, krdictUrl } from '@/lib/utils'
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
import { useAuth } from '@/components/auth-provider'
import { EditCardDialog } from '@/components/edit-card-dialog'
import { Tooltip, TooltipTrigger, TooltipContent } from '@/components/ui/tooltip'
import { useEnumLookups } from '@/components/enum-lookups-provider'
import { useCardTheme } from '@/components/card-theme-provider'
import { CardThemeDecorationBefore, CardThemeDecorationAfter } from '@/components/card-theme-decorations'

interface FlashcardProps {
  card: Card
  onReview: (rating: number) => void | Promise<void>
  onSuppress?: () => void
  onCardUpdated?: (updates: Partial<Card>) => void
}

function isHanja(char: string): boolean {
  const code = char.codePointAt(0)!
  return code >= 0x4E00 && code <= 0x9FFF
}

function HintRow({ hint, sharedChars, showTarget }: { hint: HanjaHint; sharedChars: string; showTarget: boolean }) {
  const sharedSet = new Set([...sharedChars].filter(isHanja))

  return (
    <span className="text-sm">
      {hint.hanja.split('').map((char, i) => (
        <span key={i} className={sharedSet.has(char) ? 'text-orange-700 dark:text-orange-400' : ''}>
          {char}
        </span>
      ))}
      {showTarget && hint.trans_word && <span className="text-muted-foreground/70"> ({hint.trans_word})</span>}
    </span>
  )
}

function HanjaHintText({
  hanja,
  hints,
  showEum,
  hanjaEum,
  showTarget,
}: {
  hanja: string
  hints: HanjaHint[]
  showEum: boolean
  hanjaEum?: string | null
  showTarget: boolean
}) {
  const [open, setOpen] = useState(false)

  if (hints.length === 0) {
    return (
      <>
        {hanja}{showEum && hanjaEum && ` (${hanjaEum})`}
      </>
    )
  }

  const visible = `${hanja}${showEum && hanjaEum ? ` (${hanjaEum})` : ''}`
  // base-ui tooltips emit no role and no aria-describedby by design, and the
  // content is unmounted until hover — so the gloss exists nowhere in the
  // accessibility tree unless the trigger carries it as its own name.
  const glossed = hints
    .map(h => `${h.hanja}${showTarget && h.trans_word ? ` (${h.trans_word})` : ''}`)
    .join(', ')

  return (
    <Tooltip open={open} onOpenChange={setOpen}>
      <TooltipTrigger
        delay={300}
        closeOnClick
        tabIndex={0}
        aria-label={`${visible} — hanja: ${glossed}`}
        className="cursor-help border-b border-dotted border-current"
        render={<span />}
      >
        {visible}
      </TooltipTrigger>
      <TooltipContent side="top" sideOffset={6}>
        <div className="flex flex-col gap-1">
          {hints.map((hint, i) => (
            <HintRow key={i} hint={hint} sharedChars={hanja} showTarget={showTarget} />
          ))}
        </div>
      </TooltipContent>
    </Tooltip>
  )
}

function GrammarPatternPill({
  label,
  tooltip,
  endings,
  showEndings,
}: {
  label: string
  tooltip: string
  endings?: string | null
  showEndings: boolean
}) {
  const [open, setOpen] = useState(false)

  const explanation = `${tooltip}${showEndings && endings ? ` (${endings})` : ''}`

  return (
    <Tooltip open={open} onOpenChange={setOpen}>
      <TooltipTrigger
        delay={300}
        closeOnClick
        tabIndex={0}
        aria-label={`Grammar pattern: ${label} — ${explanation}`}
        render={<span data-slot="badge" />}
        className="inline-block text-xs px-2 py-0.5 rounded-full bg-secondary text-secondary-foreground cursor-help"
      >
        {label}
      </TooltipTrigger>
      <TooltipContent side="top" sideOffset={6}>
        {explanation}
      </TooltipContent>
    </Tooltip>
  )
}

export function Flashcard({ card, onReview, onSuppress, onCardUpdated }: FlashcardProps) {
  const [input, setInput] = useState('')
  const [answered, setAnswered] = useState(false)
  const [correct, setCorrect] = useState(false)
  const [submittedAnswer, setSubmittedAnswer] = useState('')
  const [suppressing, setSuppressing] = useState(false)
  const [isAutoProgressing, setIsAutoProgressing] = useState(false)
  const [editOpen, setEditOpen] = useState(false)
  const [menuOpen, setMenuOpen] = useState(false)
  const [advancing, setAdvancing] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  const submitButtonRef = useRef<HTMLButtonElement>(null)
  const { settings } = useSettings()
  const { isAdmin } = useAuth()
  const { theme: cardTheme } = useCardTheme()
  const { pos: posLookup, speechLevel: speechLevelLookup, tense: tenseLookup, grammarPattern: grammarPatterns } = useEnumLookups()
  const hasAutoProgressedRef = useRef(false)

  const showInfinitive = (answered || isAutoProgressing) && card.pos && (card.pos === '동사' || card.pos === '형용사')

  // `advancing` guards against a second submission for the same card. Nothing
  // visible changes while the review is in flight (the outgoing card stays put
  // by design), so without this a click that looks like it did nothing invites
  // a retry that posts the review twice.
  const handleAdvance = useCallback(async () => {
    if (!answered || advancing) return
    setAdvancing(true)
    try {
      // Submit review: 1 = Again (wrong), 3 = Good (correct)
      await onReview(correct ? 3 : 1)
    } finally {
      setAdvancing(false)
    }
  }, [answered, advancing, correct, onReview])

  useEffect(() => {
    setInput('')
    setAnswered(false)
    setCorrect(false)
    setSubmittedAnswer('')
    setIsAutoProgressing(false)
    setEditOpen(false)
    setMenuOpen(false)
    setAdvancing(false)
    hasAutoProgressedRef.current = false
    inputRef.current?.focus()
    // Keyboard stays open across cards on mobile (see below), so the check
    // button can end up hidden behind it once the new card's content settles.
    submitButtonRef.current?.scrollIntoView({ block: 'nearest' })
  }, [card])

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (editOpen || menuOpen) return
      if (answered && (e.key === KEYS.ENTER || e.key === KEYS.SPACE)) {
        e.preventDefault()
        handleAdvance()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [answered, handleAdvance, editOpen, menuOpen])

  // Split sentence at target word position
  // Note: card.target is the conjugated form, but sentence might contain base form
  // We use the 'target' field from the sentence which indicates the exact match position
  const targetStart = card.sentence.indexOf(card.target)
  let before = ''
  let after = ''

  if (targetStart >= 0) {
    before = card.sentence.substring(0, targetStart)
    after = card.sentence.substring(targetStart + card.target.length)
  } else {
    // Fallback: if exact match not found, just show the full sentence
    console.warn('Target word not found in sentence:', { target: card.target, sentence: card.sentence })
    before = card.sentence
    after = ''
  }

  const krdictLink = krdictUrl(card.krdict_id)

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (answered) return
    const trimmed = input.trim()
    const isCorrect = trimmed === card.target || (card.alternatives ?? []).includes(trimmed)
    setCorrect(isCorrect)
    setSubmittedAnswer(trimmed)

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

  const handleCopyJson = () => {
    const text = JSON.stringify(card, null, 2)
    if (navigator.clipboard) {
      navigator.clipboard.writeText(text).catch((err) => {
        console.error('Error copying card JSON:', err)
      })
    } else {
      const el = document.createElement('textarea')
      el.value = text
      el.style.cssText = 'position:fixed;opacity:0'
      document.body.appendChild(el)
      el.focus()
      el.select()
      document.execCommand('copy')
      document.body.removeChild(el)
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
    <article
      data-card-theme={cardTheme.id}
      // The one stable token that changes when the card does. Nothing else
      // in the markup reliably differs between two consecutive cards, and
      // this subtree is not remounted (see App.tsx).
      data-card-id={card.card_id}
      data-answered={answered || isAutoProgressing}
      aria-label={`Review card: ${card.trans_word}`}
      className="ct-card relative w-full max-w-xl"
    >
      <CardThemeDecorationBefore themeId={cardTheme.id} />
      <UICard className="w-full">
      <CardHeader>
        <div className="flex items-center justify-between gap-2 mb-2">
          <div className="flex items-center gap-1.5">
          {card.guess_count === 0 && (
            <span data-slot="new-badge" className="inline-block text-xs px-2 py-0.5 rounded-full bg-orange-100 dark:bg-orange-900/30 text-orange-700 dark:text-orange-400 font-medium">
              {cardTheme.newLabel}
            </span>
          )}
          </div>
          <div className="flex flex-wrap items-center justify-center gap-1.5 flex-1">
          {/*
            Visually these badges are distinguished by position and context;
            as bare text they'd be three unlabelled values in a row, so each
            names the dimension it reports.
          */}
          {card.pos && posLookup[card.pos] && (
            <span data-slot="badge" className="inline-block text-xs px-2 py-0.5 rounded-full bg-secondary text-secondary-foreground">
              <span className="sr-only">Part of speech: </span>
              {posLookup[card.pos].label}
            </span>
          )}
          {card.speech_level && speechLevelLookup[card.speech_level] && (
            <span data-slot="badge" className="inline-block text-xs px-2 py-0.5 rounded-full bg-secondary text-secondary-foreground">
              <span className="sr-only">Speech level: </span>
              {speechLevelLookup[card.speech_level].label}
            </span>
          )}
          {card.tense && tenseLookup[card.tense] && (
            <span data-slot="badge" className="inline-block text-xs px-2 py-0.5 rounded-full bg-secondary text-secondary-foreground">
              <span className="sr-only">Tense: </span>
              {tenseLookup[card.tense].label}
            </span>
          )}
          {card.grammar_pattern && grammarPatterns[card.grammar_pattern] && (
            <GrammarPatternPill
              label={grammarPatterns[card.grammar_pattern].label}
              tooltip={grammarPatterns[card.grammar_pattern].tooltip ?? ''}
              endings={grammarPatterns[card.grammar_pattern].endings}
              showEndings={answered || isAutoProgressing}
            />
          )}
          </div>
          <DropdownMenu onOpenChange={setMenuOpen}>
            <DropdownMenuTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label="Card options"
                  className="h-8 w-8 text-muted-foreground hover:text-card-foreground"
                  disabled={suppressing}
                >
                  <MoreVertical className="h-4 w-4" />
                </Button>
              }
            />
            <DropdownMenuContent align="end" className="min-w-50">
              {isAdmin && (
                <DropdownMenuItem onClick={() => setEditOpen(true)}>
                  Edit card
                </DropdownMenuItem>
              )}
              {krdictLink && (
                <DropdownMenuItem
                  render={<a href={krdictLink} target="_blank" rel="noreferrer" />}
                >
                  Look up in KRDICT
                </DropdownMenuItem>
              )}
              <DropdownMenuItem onClick={handleCopyJson}>
                Copy card as JSON
              </DropdownMenuItem>
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
          {/* The document is lang="en"; this subtree and the answer field are Korean. */}
          <p lang="ko" className="text-2xl md:text-3xl font-semibold leading-relaxed text-center">
            {before}
            <span className="inline-flex flex-col items-center relative pt-5">
              {card.hanja && (
                <span className="text-sm text-muted-foreground/60 whitespace-nowrap absolute top-0 left-1/2 -translate-x-1/2 select-none">
                  <HanjaHintText
                    hanja={card.hanja}
                    hints={card.hanja_hints ?? []}
                    showEum={answered || isAutoProgressing}
                    hanjaEum={card.hanja_eum}
                    showTarget={answered || isAutoProgressing}
                  />
                </span>
              )}
              {isAutoProgressing ? (
                // Show green answer during auto-progress for positive feedback
                <span className="text-green-600">{submittedAnswer}</span>
              ) : answered ? (
                correct ? (
                  <span className="text-green-600">{submittedAnswer}</span>
                ) : (
                  // Per-character correctness is otherwise carried only by the
                  // text colour; data-correct states it outright.
                  <span className="inline-flex flex-wrap items-baseline gap-0">
                    {submittedAnswer.split('').map((char, i) => (
                      <span
                        key={i}
                        data-correct={char === card.target[i]}
                        className={char === card.target[i] ? 'text-green-600' : 'text-destructive'}
                      >
                        {char}
                      </span>
                    ))}
                    <span className="text-muted-foreground/50 ml-1 select-none">({card.target})</span>
                  </span>
                )
              ) : null}
              {/*
                Always mounted and never blurred, even after grading — on mobile,
                unmounting/refocusing this field dismisses and reopens the on-screen
                keyboard between cards. Hidden with sr-only (not removed) once
                answered, so focus (and the keyboard) survive into the next card.
              */}
              <input
                ref={inputRef}
                type="text"
                lang="ko"
                aria-label="Answer"
                value={input}
                onChange={(e) => {
                  if (!answered && !isAutoProgressing) setInput(e.target.value)
                }}
                onFocus={() => submitButtonRef.current?.scrollIntoView({ block: 'nearest' })}
                style={{ width: `${Math.max(input.length, 2)}em` }}
                className={cn(
                  'flex-none bg-transparent border-0 border-b-2 border-card-foreground/30',
                  'text-center text-2xl md:text-3xl font-semibold',
                  'outline-none pb-0.5',
                  'focus:border-primary',
                  !settings.autoProgressOnCorrect && 'transition-colors',
                  (answered || isAutoProgressing) && 'sr-only',
                )}
                autoFocus
              />
              {(answered || isAutoProgressing) && showInfinitive && (
                <span className="text-xs text-muted-foreground/60 mt-1 select-none">({card.word})</span>
              )}
            </span>
            {after}
          </p>
        </form>

        <p className="text-sm text-muted-foreground text-center mt-2" title={card.trans_dfn ?? undefined}>
          <span className="sr-only">Word meaning: </span>
          {card.trans_word}
          {/* Otherwise reachable only as a title attribute, i.e. only on hover. */}
          {card.trans_dfn && <span className="sr-only"> — {card.trans_dfn}</span>}
        </p>
        <p className="text-xs text-muted-foreground/70 text-center italic">
          <span className="sr-only">Sentence translation: </span>
          {card.sentence_translation}
        </p>
      </CardHeader>

      {isAdmin && (
        <EditCardDialog
          open={editOpen}
          onOpenChange={setEditOpen}
          card={card}
          onSaved={onCardUpdated}
        />
      )}

      {/*
        During auto-progress the footer keeps its box so the card doesn't
        collapse, but `inert` takes it out of the accessibility tree and off
        the hit-testing path — otherwise it advertises a "Next" button that
        cannot be clicked.
      */}
      <CardFooter
        className={cn("flex-col gap-3", isAutoProgressing && "invisible")}
        inert={isAutoProgressing}
      >
        {!answered && !isAutoProgressing ? (
          <Button ref={submitButtonRef} type="submit" onClick={handleSubmit} className="w-full">
            {cardTheme.checkLabel}
          </Button>
        ) : (
          <>
            <p role="status" className={cn("text-sm font-medium", correct ? "text-green-600" : "text-destructive")}>
              {correct
                ? `Correct! The answer was ${card.target}.`
                : `Incorrect. You typed ${submittedAnswer || '(nothing)'}; the answer was ${card.target}.`}
            </p>
            <Button onClick={handleAdvance} disabled={advancing} variant="outline" className="w-full">
              {advancing ? 'Loading next card…' : 'Next'}
            </Button>
          </>
        )}
      </CardFooter>
      </UICard>
      <CardThemeDecorationAfter themeId={cardTheme.id} />
    </article>
  )
}
