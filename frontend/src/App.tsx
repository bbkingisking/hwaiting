import { useState, useEffect, useRef } from 'react'
import { DotLottieReact } from '@lottiefiles/dotlottie-react'
import { Flashcard } from '@/components/flashcard'
import { ThemeProvider } from '@/components/theme-provider'
import { CardThemeProvider, useCardTheme } from '@/components/card-theme-provider'
import { SettingsProvider } from '@/components/settings-provider'
import { EnumLookupsProvider } from '@/components/enum-lookups-provider'

import { AuthProvider, useAuth } from '@/components/auth-provider'
import { AuthDialog } from '@/components/auth-dialog'
import { AppHeader } from '@/components/app-header'
import { StatusIndicator } from '@/components/status-indicator'
import { DebugStatusBar } from '@/components/debug-status-bar'
import { getNextCard, checkAnswer, ApiError } from '@/lib/api'
import type { NextCardEnvelope, AdminCard } from '@/lib/api'
import type { CardPrompt, CardReveal } from '@/lib/types'
import { formatTimeUntil } from '@/lib/utils'

// Tracks a background fetch for the next card.
type PrefetchSlot = {
  abort: AbortController
  promise: Promise<NextCardEnvelope | null>
  result: NextCardEnvelope | null
}

// The subset of AdminCard's editable fields that CardPrompt actually has -
// see handleCardUpdated below.
const CARD_PROMPT_FIELDS = new Set([
  'pos', 'origin_type', 'grade', 'hanja', 'trans_word', 'trans_dfn',
  'sentence_translation', 'speech_level', 'tense', 'grammar_pattern',
])

function AppContent() {
  const [card, setCard] = useState<CardPrompt | null>(null)
  const [loading, setLoading] = useState(false)
  // Fetch failures and answer-check failures are different concerns with
  // different lifetimes: advanceToNextCard clears the fetch error as it starts
  // a new fetch, which would otherwise destroy a submission error raised
  // moments earlier in the same batch.
  const [error, setError] = useState<string | null>(null)
  const [submitError, setSubmitError] = useState<string | null>(null)
  // Checking an answer is the one network call left in the review loop that
  // can't be hidden behind a prefetch - the DOM stays put while it's in
  // flight (Flashcard disables the Check button itself), and this publishes
  // that as aria-busy rather than as a visible loading state.
  const [checking, setChecking] = useState(false)
  const [lastCheckMs, setLastCheckMs] = useState<number | null>(null)
  const [noCards, setNoCards] = useState(false)
  const [nextDueAt, setNextDueAt] = useState<string | null>(null)
  const [authDialogOpen, setAuthDialogOpen] = useState(false)
  const [statsKey, setStatsKey] = useState(0)

  const { isAuthenticated } = useAuth()
  const { cardThemeId } = useCardTheme()

  const prefetchRef = useRef<PrefetchSlot | null>(null)

  useEffect(() => {
    if (!isAuthenticated) {
      setAuthDialogOpen(true)
    } else {
      loadCardCold()
    }
  }, [isAuthenticated])

  // Cancel any in-flight prefetch and clear the cached prefetched card.
  const cancelPrefetch = () => {
    if (prefetchRef.current) {
      prefetchRef.current.abort.abort()
      prefetchRef.current = null
    }
  }

  // Kick off a background fetch for the card after `currentCardId`.
  // Fires immediately — no waitFor. The `exclude` parameter prevents
  // the API from returning the current card.
  const startPrefetch = (currentCardId: number): PrefetchSlot => {
    cancelPrefetch()

    const controller = new AbortController()
    const slot: PrefetchSlot = {
      abort: controller,
      result: null,
      promise: Promise.resolve(null),
    }

    slot.promise = (async () => {
      try {
        const envelope = await getNextCard({
          excludeCardId: currentCardId,
          signal: controller.signal,
        })
        if (prefetchRef.current === slot) {
          slot.result = envelope
        }
        return envelope
      } catch (err) {
        if (err instanceof Error && err.name !== 'AbortError') {
          console.debug('Prefetch failed:', err)
        }
        return null
      }
    })()

    prefetchRef.current = slot
    return slot
  }

  // Cold load: shows a loading state. Used for initial load, after
  // errors, after suppress, and when cards become available again.
  const loadCardCold = async () => {
    cancelPrefetch()
    setLoading(true)
    setError(null)
    setNoCards(false)
    try {
      const envelope = await getNextCard()
      if (envelope.card) {
        setCard(envelope.card)
        setNoCards(false)
        startPrefetch(envelope.card.card_id)
      } else {
        setCard(null)
        setNoCards(true)
        setNextDueAt(envelope.next_due_at ?? null)
      }
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.message)
      } else {
        setError('Failed to load card')
      }
      console.error('Error fetching card:', err)
    } finally {
      setLoading(false)
    }
  }

  // Advance to the next card. Uses the prefetched card if ready;
  // otherwise fetches fresh. Never sets loading=true during a
  // card-to-card transition — the old card stays visible until
  // the new one arrives, so there's no flash.
  const advanceToNextCard = async () => {
    setError(null)
    setNoCards(false)

    const slot = prefetchRef.current

    // Fast path: prefetch already resolved with a card.
    if (slot?.result?.card) {
      const nextCard = slot.result.card
      prefetchRef.current = null
      setCard(nextCard)
      setNoCards(false)
      startPrefetch(nextCard.card_id)
      return
    }

    // Any other case (in-flight, null result, no prefetch): fetch
    // fresh. Old card stays visible during the fetch.
    prefetchRef.current = null
    try {
      const envelope = await getNextCard()
      if (envelope.card) {
        setCard(envelope.card)
        setNoCards(false)
        startPrefetch(envelope.card.card_id)
      } else {
        setCard(null)
        setNoCards(true)
        setNextDueAt(envelope.next_due_at ?? null)
      }
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.message)
      } else {
        setError('Failed to load next card')
      }
      console.error('Error fetching next card:', err)
    }
  }

  // Grades `answer` against the current card server-side and records the
  // FSRS review in the same call - see backend/src/cards.rs's check_answer.
  // Resolves with the reveal rather than stashing it in `card` state here:
  // `card` only ever holds the prompt (see its type above), and handing the
  // reveal back as a plain return value lets Flashcard set it as its own
  // local state in the same synchronous continuation as `answered`/
  // `correct`, instead of racing a second, independently-timed update to
  // this component's state against Flashcard's re-render (see CardReveal's
  // doc comment in lib/types.ts for the bug that came from doing that).
  // Throws on failure so Flashcard's handleSubmit can leave the card
  // unanswered and let the user retry, rather than showing a reveal that
  // never arrived.
  const handleCheck = async (answer: string): Promise<{ correct: boolean; reveal: CardReveal }> => {
    if (!card) throw new Error('No card to check')
    const cardId = card.card_id

    setSubmitError(null)
    setChecking(true)

    try {
      const startedAt = performance.now()
      // CheckResponse is CardReveal flattened with `correct` (see
      // #[serde(flatten)] on cards::CheckResponse) - split them back apart.
      const { correct, ...reveal } = await checkAnswer(cardId, answer)
      setLastCheckMs(performance.now() - startedAt)
      // Bumped only now, not before the request: StatusIndicator remounts
      // on this key change and fetches stats on mount, and check_answer is
      // what actually records the review server-side - firing this before
      // the request even started meant the stats fetch usually won the race
      // against its own write and rendered the accuracy from before this
      // card, correcting itself only on the next unrelated fetch (a manual
      // refresh, or the 30s poll).
      setStatsKey((prev) => prev + 1)
      return { correct, reveal }
    } catch (err) {
      if (err instanceof ApiError) {
        setSubmitError(`Failed to check answer: ${err.message}`)
      } else {
        setSubmitError('Failed to check answer')
      }
      console.error('Error checking answer:', err)
      throw err
    } finally {
      setChecking(false)
    }
  }

  const handleSuppress = async () => {
    await loadCardCold()
    setStatsKey((prev) => prev + 1)
  }

  // EditCardDialog edits the backend's canonical full-card shape (see its
  // doc comment in lib/api.ts), most of which - word, definition, sentence,
  // target, alternatives, hanja_eum - isn't part of `card` (CardPrompt) at
  // all; that half lives only in Flashcard's local `reveal` state, which
  // patches itself directly from the same `updates` (see flashcard.tsx's own
  // onSaved wrapper). Only the fields CardPrompt actually has are relevant
  // here.
  const handleCardUpdated = (updates: Partial<AdminCard>) => {
    const filtered = Object.fromEntries(
      Object.entries(updates).filter(([key, v]) => CARD_PROMPT_FIELDS.has(key) && v !== undefined)
    ) as Partial<CardPrompt>
    setCard(prev => prev ? { ...prev, ...filtered } : prev)
  }

  return (
    <>
      <AuthDialog open={authDialogOpen} onOpenChange={setAuthDialogOpen} />
      <AppHeader />
      <main
        data-card-theme={cardThemeId}
        aria-busy={loading || checking}
        className="ct-page min-h-screen flex flex-col items-center justify-center p-6"
      >
        <h1 className="sr-only">Hwaiting — Korean review</h1>
        {!isAuthenticated ? (
          <div role="status" className="text-center text-muted-foreground">
            <p>Please log in to continue</p>
          </div>
        ) : loading ? (
          <div role="status" className="text-center text-muted-foreground">
            <p>Loading card...</p>
          </div>
        ) : noCards && !card ? (
          <div role="status" className="text-center text-muted-foreground">
            <div aria-hidden className="w-64 mx-auto mb-4">
              <DotLottieReact
                src="/Taegeukgi.json"
                loop
                autoplay
              />
            </div>
            <p className="mb-2">No cards to review right now</p>
            {nextDueAt && (() => {
              const formatted = formatTimeUntil(nextDueAt)
              return formatted ? <p className="text-sm">Next in {formatted}</p> : null
            })()}
          </div>
        ) : error && !card ? (
          <div className="text-center">
            <p role="alert" className="text-destructive mb-4">{error}</p>
            <button
              onClick={() => loadCardCold()}
              className="text-sm text-primary hover:underline"
            >
              Try again
            </button>
          </div>
        ) : card ? (
          <>
            {(error || submitError) && (
              <p role="alert" className="text-destructive text-sm mb-4">{error ?? submitError}</p>
            )}
            {/*
              Deliberately NOT keyed by card_id. Keying would remount the
              subtree on every card, which unmounts the answer input — and
              that field is kept mounted on purpose so the mobile keyboard
              survives the transition (see flashcard.tsx). The card's
              identity is published as data-card-id instead.
            */}
            <Flashcard
              card={card}
              onCheck={handleCheck}
              onAdvance={advanceToNextCard}
              onSuppress={handleSuppress}
              onCardUpdated={handleCardUpdated}
            />
          </>
        ) : (
          <div role="status" className="text-center text-muted-foreground">
            <p>No cards available</p>
          </div>
        )}
      </main>
      {isAuthenticated && <StatusIndicator key={statsKey} onCardsAvailable={loadCardCold} />}
      {isAuthenticated && <DebugStatusBar lastCheckMs={lastCheckMs} />}
    </>
  )
}

function App() {
  return (
    <ThemeProvider>
      <CardThemeProvider>
        <AuthProvider>
          <SettingsProvider>
            <EnumLookupsProvider>
              <AppContent />
            </EnumLookupsProvider>
          </SettingsProvider>
        </AuthProvider>
      </CardThemeProvider>
    </ThemeProvider>
  )
}

export default App
