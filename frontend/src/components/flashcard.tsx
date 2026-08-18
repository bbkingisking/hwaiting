import { useState, useEffect, useRef, useCallback } from 'react'
import type { CardPrompt, CardReveal, HanjaHint } from '@/lib/types'
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
import { suppressCard, CARD_BACK_FIELDS, type AdminCard } from '@/lib/api'
import { useAuth } from '@/components/auth-provider'
import { EditCardDialog } from '@/components/edit-card-dialog'
import { InflectionsDialog } from '@/components/inflections-dialog'
import { Tooltip, TooltipTrigger, TooltipContent } from '@/components/ui/tooltip'
import { useFieldValues } from '@/components/field-values-provider'
import { useCardTheme } from '@/components/card-theme-provider'
import { CardThemeDecorationBefore, CardThemeDecorationAfter } from '@/components/card-theme-decorations'

interface FlashcardProps {
  card: CardPrompt
  // Grades `answer` server-side and records the FSRS review; resolves with
  // whether it was correct and the card's secret half. Callers hold the
  // reveal themselves (see the `reveal` state below) rather than expecting
  // it to show up in `card` - see CardReveal's doc comment in lib/types.ts.
  onCheck: (answer: string) => Promise<{ correct: boolean; reveal: CardReveal }>
  // Moves to the next card. Never itself talks to the network on the warm
  // path - see startPrefetch in App.tsx.
  onAdvance: () => void | Promise<void>
  onSuppress?: () => void
  onCardUpdated?: (updates: Partial<AdminCard>) => void
}

// CARD_BACK_FIELDS (lib/api.ts) is the subset of AdminCard's editable
// fields that CardReveal actually has - see the onSaved wrapper below.

// Pre-answer, only the hanja *characters* of each hint are known (see
// CardPrompt::hanja_hint_words) - the reading/gloss give the answer away, so
// they're absent until `reveal` arrives. Stand in with gloss-free hints
// until then; `showTarget` already keeps callers from reading `.trans_word`
// before that point.
function displayHanjaHints(card: CardPrompt, reveal: CardReveal | null): HanjaHint[] {
  if (reveal) return reveal.hanja_hints
  return (card.hanja_hint_words ?? []).map((hanja) => ({ hanja, word: '', trans_word: null }))
}

// EditCardDialog edits the backend's canonical full-card shape (see its doc
// comment in lib/api.ts), not the review card's CardPrompt/CardReveal split.
// Only called once `reveal` is set (see the "Edit card" menu item below).
function toAdminCard(card: CardPrompt, reveal: CardReveal): AdminCard {
  return {
    card_id: card.card_id,
    krdict_id: card.krdict_id,
    word: reveal.word,
    definition: reveal.definition,
    pos: card.pos,
    origin_type: card.origin_type,
    hanja: card.hanja,
    grade: card.grade,
    trans_word: card.trans_word,
    trans_dfn: card.trans_dfn,
    sentence: reveal.sentence,
    sentence_before: card.sentence_before,
    sentence_after: card.sentence_after,
    sentence_translation: card.sentence_translation,
    target: reveal.target,
    alternatives: reveal.alternatives,
    speech_level: card.speech_level,
    tense: card.tense,
    grammar_pattern: card.grammar_pattern,
  }
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

type HanjaHintParams = {
  hanja: string
  hints: HanjaHint[]
  showEum: boolean
  // The reading of `hanja` - Korean orthography is phonetic, so that's just
  // this card's own `word` (see backend's dropped-hanja_eum migration).
  word?: string | null
  showTarget: boolean
}

// What the hint says, as a single string. The visible hint is a positioned
// span with no role, so its characters merge into the surrounding sentence as
// an anonymous text run — present in a raw text dump, absent from the
// accessibility tree. This is the name that gives it a node of its own.
//
// Respects showEum/showTarget for the same reason the visible hint does: the
// reading and the gloss give the answer away before the card is graded.
function hanjaHintLabel({ hanja, hints, showEum, word, showTarget }: HanjaHintParams): string {
  const base = `${hanja}${showEum && word ? ` (${word})` : ''}`
  if (hints.length === 0) return `Hanja hint: ${base}`
  const glossed = hints
    .map(h => `${h.hanja}${showTarget && h.trans_word ? ` (${h.trans_word})` : ''}`)
    .join(', ')
  return `Hanja hint: ${base} — ${glossed}`
}

function HanjaHintText({
  hanja,
  hints,
  showEum,
  word,
  showTarget,
}: HanjaHintParams) {
  const [open, setOpen] = useState(false)

  if (hints.length === 0) {
    return (
      <>
        {hanja}{showEum && word && ` (${word})`}
      </>
    )
  }

  const visible = `${hanja}${showEum && word ? ` (${word})` : ''}`

  return (
    <Tooltip open={open} onOpenChange={setOpen}>
      {/*
        No aria-label here: the wrapping role="note" already carries the full
        gloss, and naming the trigger too would say the same thing twice.
        tabIndex stays so the visual tooltip is reachable without a mouse —
        base-ui tooltips expose no role and unmount their content until hover.
      */}
      <TooltipTrigger
        delay={300}
        closeOnClick
        tabIndex={0}
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
}: {
  label: string
  tooltip: string
  // The pattern's conjugation endings - as spoiling as `target` for any card
  // that uses this pattern, so it only arrives with the reveal (see
  // CardReveal::grammar_pattern_endings); its mere presence is the gate.
  endings?: string | null
}) {
  const [open, setOpen] = useState(false)

  const explanation = `${tooltip}${endings ? ` (${endings})` : ''}`

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

export function Flashcard({ card, onCheck, onAdvance, onSuppress, onCardUpdated }: FlashcardProps) {
  const [input, setInput] = useState('')
  const [checking, setChecking] = useState(false)
  const [answered, setAnswered] = useState(false)
  const [correct, setCorrect] = useState(false)
  // The card's secret half, set in the same synchronous continuation as
  // `answered`/`correct` (see handleSubmit) - never sourced from `card`
  // itself. See CardReveal's doc comment in lib/types.ts for why.
  const [reveal, setReveal] = useState<CardReveal | null>(null)
  const [submittedAnswer, setSubmittedAnswer] = useState('')
  const [suppressing, setSuppressing] = useState(false)
  const [isAutoProgressing, setIsAutoProgressing] = useState(false)
  const [editOpen, setEditOpen] = useState(false)
  const [menuOpen, setMenuOpen] = useState(false)
  const [inflectionsOpen, setInflectionsOpen] = useState(false)
  const [advancing, setAdvancing] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  const submitButtonRef = useRef<HTMLButtonElement>(null)
  const { settings } = useSettings()
  const { isAdmin } = useAuth()
  const { theme: cardTheme } = useCardTheme()
  const { pos: posLookup, speechLevel: speechLevelLookup, tense: tenseLookup, grammarPattern: grammarPatterns } = useFieldValues()
  const hasAutoProgressedRef = useRef(false)

  const showInfinitive = (answered || isAutoProgressing) && card.pos && (card.pos === '동사' || card.pos === '형용사')

  // `advancing` guards against a second click while a cold-path fetch for the
  // next card is in flight (the warm path, served from the prefetch buffer,
  // resolves synchronously). Nothing visible changes meanwhile (the outgoing
  // card stays put by design), so without this a click that looks like it
  // did nothing invites a retry.
  const handleAdvance = useCallback(async () => {
    if (!answered || advancing) return
    setAdvancing(true)
    try {
      await onAdvance()
    } finally {
      setAdvancing(false)
    }
  }, [answered, advancing, onAdvance])

  useEffect(() => {
    setInput('')
    setChecking(false)
    setAnswered(false)
    setCorrect(false)
    setReveal(null)
    setSubmittedAnswer('')
    setIsAutoProgressing(false)
    setEditOpen(false)
    setMenuOpen(false)
    setInflectionsOpen(false)
    setAdvancing(false)
    hasAutoProgressedRef.current = false
    inputRef.current?.focus()
    // Keyboard stays open across cards on mobile (see below), so the check
    // button can end up hidden behind it once the new card's content settles.
    submitButtonRef.current?.scrollIntoView({ block: 'nearest' })
    // Keyed on card_id, not `card` itself: an admin edit saved mid-review
    // (see onCardUpdated below) also produces a new `card` object for the
    // *same* card, which must not look like a new card arriving and wipe
    // out the answer/reveal being displayed.
  }, [card.card_id])

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (editOpen || menuOpen || inflectionsOpen) return
      if (answered && (e.key === KEYS.ENTER || e.key === KEYS.SPACE)) {
        e.preventDefault()
        handleAdvance()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [answered, handleAdvance, editOpen, menuOpen, inflectionsOpen])

  // The backend splits `sentence` around `target` for us (see
  // cards::split_sentence) - it owns both strings and is the only place that
  // needs to search one for the other.
  const before = card.sentence_before
  const after = card.sentence_after

  const krdictLink = krdictUrl(card.krdict_id)

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (answered || checking) return
    const trimmed = input.trim()
    setSubmittedAnswer(trimmed)
    setChecking(true)
    try {
      const { correct: isCorrect, reveal: revealData } = await onCheck(trimmed)
      setCorrect(isCorrect)
      setReveal(revealData)

      // Auto-progress if correct and setting is enabled
      if (isCorrect && settings.autoProgressOnCorrect && !hasAutoProgressedRef.current) {
        hasAutoProgressedRef.current = true
        setIsAutoProgressing(true)
        // Use configurable delay before progressing
        setTimeout(() => {
          onAdvance()
        }, settings.autoProgressDelay)
      } else {
        // Only set answered state if not auto-progressing
        setAnswered(true)
      }
    } catch (err) {
      // Checking failed (network, etc.) - leave the card unanswered so the
      // user can retry, rather than showing a reveal that never arrived.
      console.error('Error checking answer:', err)
      setSubmittedAnswer('')
    } finally {
      setChecking(false)
    }
  }

  const handleCopyJson = () => {
    const text = JSON.stringify(reveal ? { ...card, ...reveal } : card, null, 2)
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
            reveal && reveal.inflections.length > 0 ? (
              // Conjugation table is exactly as spoiling as `target` (see
              // CardReveal::inflections), so the pill only becomes clickable
              // once `reveal` has arrived - pre-reveal it's the same inert
              // span as every other badge here.
              <button
                type="button"
                data-slot="badge"
                onClick={() => setInflectionsOpen(true)}
                aria-label={`Part of speech: ${posLookup[card.pos].label}. Show conjugation table.`}
                className="inline-block text-xs px-2 py-0.5 rounded-full bg-secondary text-secondary-foreground hover:bg-secondary/70 cursor-pointer"
              >
                {posLookup[card.pos].label}
              </button>
            ) : (
              <span data-slot="badge" className="inline-block text-xs px-2 py-0.5 rounded-full bg-secondary text-secondary-foreground">
                <span className="sr-only">Part of speech: </span>
                {posLookup[card.pos].label}
              </span>
            )
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
              endings={reveal?.grammar_pattern_endings}
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
              {/*
                Gated on `reveal`, not just `isAdmin`: CardReveal's fields
                only exist once the card is graded (see lib/types.ts) -
                editing it earlier would open the dialog on a blank record.
              */}
              {isAdmin && reveal && (
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
                // role + aria-label only — the classes here are what keep the
                // hint centred above the blank as the blank moves around the
                // sentence, so they are deliberately left alone.
                <span
                  role="note"
                  aria-label={hanjaHintLabel({
                    hanja: card.hanja,
                    hints: displayHanjaHints(card, reveal),
                    showEum: answered || isAutoProgressing,
                    word: reveal?.word,
                    showTarget: answered || isAutoProgressing,
                  })}
                  className="text-sm text-muted-foreground/60 whitespace-nowrap absolute top-0 left-1/2 -translate-x-1/2 select-none"
                >
                  <HanjaHintText
                    hanja={card.hanja}
                    hints={displayHanjaHints(card, reveal)}
                    showEum={answered || isAutoProgressing}
                    word={reveal?.word}
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
                  // text colour; data-correct states it outright. `reveal!` is
                  // safe throughout this branch - it only renders once
                  // `answered`, which handleSubmit only ever sets alongside
                  // `reveal` itself, in the same synchronous continuation.
                  <span className="inline-flex flex-wrap items-baseline gap-0">
                    {submittedAnswer.split('').map((char, i) => (
                      <span
                        key={i}
                        data-correct={char === reveal!.target[i]}
                        className={char === reveal!.target[i] ? 'text-green-600' : 'text-destructive'}
                      >
                        {char}
                      </span>
                    ))}
                    <span className="text-muted-foreground/50 ml-1 select-none">({reveal!.target})</span>
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
                <span className="text-xs text-muted-foreground/60 mt-1 select-none">({reveal?.word})</span>
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

      {/* Mounting is gated the same as the menu item that opens it (above) -
          `reveal` is guaranteed non-null here. */}
      {isAdmin && reveal && (
        <EditCardDialog
          open={editOpen}
          onOpenChange={setEditOpen}
          card={toAdminCard(card, reveal)}
          onSaved={(updates) => {
            // `card` (App.tsx state) only holds CardPrompt's fields; patch
            // the CardReveal-shaped rest directly into our own local state
            // rather than routing it through a prop round-trip.
            const revealUpdates = Object.fromEntries(
              Object.entries(updates).filter(([key, v]) => CARD_BACK_FIELDS.has(key) && v !== undefined)
            ) as Partial<CardReveal>
            setReveal((prev) => (prev ? { ...prev, ...revealUpdates } : prev))
            onCardUpdated?.(updates)
          }}
        />
      )}

      {/* Same mounting gate as EditCardDialog above - the pill that opens
          this is only clickable once `reveal` exists (see the pos badge). */}
      {reveal && card.pos && (
        <InflectionsDialog
          open={inflectionsOpen}
          onOpenChange={setInflectionsOpen}
          word={reveal.word}
          pos={card.pos}
          inflections={reveal.inflections}
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
          <Button ref={submitButtonRef} type="submit" onClick={handleSubmit} disabled={checking} className="w-full">
            {checking ? 'Checking…' : cardTheme.checkLabel}
          </Button>
        ) : (
          <>
            <p role="status" className={cn("text-sm font-medium", correct ? "text-green-600" : "text-destructive")}>
              {correct
                ? `Correct! The answer was ${reveal?.target}.`
                : `Incorrect. You typed ${submittedAnswer || '(nothing)'}; the answer was ${reveal?.target}.`}
              {reveal && reveal.alternatives.length > 0 && (
                <span className="block font-normal text-muted-foreground text-xs mt-0.5">
                  Also accepted: {reveal.alternatives.join(', ')}
                </span>
              )}
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
