import { canKeepSuccessorAfterReview } from '@/lib/review-events'
import { subscribeLiveEvents, type ReceivedEvent } from '@/lib/live-events'
import { createRefreshTask } from '@/lib/refresh-task'
import { useState, useEffect, useEffectEvent, useRef } from 'react'
import { DotLottieReact } from '@lottiefiles/dotlottie-react'
import { Flashcard } from '@/components/flashcard'
import { ThemeProvider } from '@/components/theme-provider'
import { CardThemeProvider, useCardTheme } from '@/components/card-theme-provider'
import { SettingsProvider } from '@/components/settings-provider'
import { FieldValuesProvider } from '@/components/field-values-provider'

import { AuthProvider, useAuth } from '@/components/auth-provider'
import { AuthDialog } from '@/components/auth-dialog'
import { AppHeader } from '@/components/app-header'
import { StatusIndicator } from '@/components/status-indicator'
import { DebugStatusBar } from '@/components/debug-status-bar'
import { getNextCard, getCardState, checkAnswer, subscribeReviewQueueChanges, ApiError } from '@/lib/api'
import type { NextCardEnvelope, AdminCard, ReviewQueueChange } from '@/lib/api'
import type { CardPrompt, CardReveal } from '@/lib/types'
import { NextDueCountdown } from '@/components/next-due-countdown'

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
  const [statsKey, setStatsKey] = useState(0)
  const [staleReason, setStaleReason] = useState<string | null>(null)
  const currentCard = useRef(card)
  currentCard.current = card
  const acceptedReview = useRef<{ cardId: number; revision: number } | null>(null)
  const acceptedContent = useRef<{ cardId: number; revision: number } | null>(null)
  const checkingRef = useRef(false)
  const submissionVersion = useRef(0)
  const allowCurrentCandidate = useRef(false)
  const refillTask = useRef<ReturnType<typeof createRefreshTask> | null>(null)
  const validationTask = useRef<ReturnType<typeof createRefreshTask> | null>(null)
  useEffect(() => {
    setStaleReason(null)
    acceptedReview.current = null
    acceptedContent.current = null
    allowCurrentCandidate.current = false
  }, [card?.card_id, card?.sentence_id, card?.review_revision, card?.content_revision])

  const { isAuthenticated } = useAuth()
  const { cardThemeId } = useCardTheme()

  const prefetchRef = useRef<PrefetchSlot | null>(null)
  const coldLoadRef = useRef<Promise<boolean> | null>(null)
  const cardRequestId = useRef(0)
  useEffect(() => {
    if (isAuthenticated) {
      loadCardCold()
    }
    return () => { ++cardRequestId.current; ++submissionVersion.current; cancelPrefetch() }
  }, [isAuthenticated])

  const handleQueueChanged = useEffectEvent((change: ReviewQueueChange) => {
    if (!isAuthenticated) return
    cancelPrefetch()
    setStatsKey(prev => prev + 1)
    if (change === 'import') {
      setCard(null)
      setSubmitError(null)
      setLastCheckMs(null)
    }
    if (change === 'import' || !card || loading) {
      loadCardCold()
    } else {
      refillTask.current?.request()
    }
  })

  useEffect(() => subscribeReviewQueueChanges(handleQueueChanged), [])

  // Remote reviews complete this person's attempt on another device. Discard
  // that attempt automatically; own echoes leave the HTTP-response flow alone.
  // Other changes preserve active work and replenish the disposable successor.
  const handleLiveEvent = useEffectEvent((event: ReceivedEvent) => {
    if (!isAuthenticated || event.own) return
    if (event.type === 'settings-changed' && !event.queue_affected) return
    setStatsKey(value => value + 1)
    const active = currentCard.current
    if (event.type !== 'card-reviewed' || !canKeepSuccessorAfterReview(prefetchRef.current?.result?.card, event.card_id)) {
      cancelPrefetch()
    }
    if (event.type === 'card-reviewed' && active?.card_id === event.card_id && event.review_revision > active.review_revision) {
      const accepted = acceptedReview.current
      if (!accepted || accepted.cardId !== active.card_id || accepted.revision < event.review_revision) {
        discardReviewedCard()
        return
      }
    } else if (event.type === 'queue-changed' && event.suppressed && active?.card_id === event.card_id) {
      setStaleReason('This card was suppressed elsewhere. Continue to the next card.')
    } else if (event.type === 'card-updated' && active?.card_id === event.card_id) {
      allowCurrentCandidate.current = true
      setStaleReason('This card was edited elsewhere. Continue to load a fresh card.')
    } else if (event.type === 'resync-required' || event.type === 'state-reset') {
      validationTask.current?.request()
    }
    if (!active) validationTask.current?.request()
    else if (!prefetchRef.current) refillTask.current?.request()
  })

  const discardReviewedCard = () => {
    ++submissionVersion.current
    checkingRef.current = false
    setChecking(false)
    setSubmitError(null)
    setLastCheckMs(null)
    setStaleReason(null)
    currentCard.current = null
    setCard(null)
    // A ready, still-eligible successor is promoted synchronously. Otherwise
    // show loading, never leave an already-reviewed card available to submit.
    setLoading(true)
    void advanceToNextCard().catch(() => {
      validationTask.current?.request()
    })
  }

  useEffect(() => {
    if (!isAuthenticated) return
    const task = createRefreshTask(async current => {
      const active = currentCard.current
      if (!active) {
        if (!await loadCardCold()) throw new Error('Queue refresh failed')
        return
      }
      if (checkingRef.current) throw new Error('Wait for pending review')
      const reviewAtStart = acceptedReview.current
      const contentAtStart = acceptedContent.current
      const state = await getCardState(active.card_id)
      if (!current() || currentCard.current !== active) return
      if (checkingRef.current || acceptedReview.current !== reviewAtStart || acceptedContent.current !== contentAtStart) {
        throw new Error('Local mutation superseded card-state read')
      }
      const accepted = acceptedReview.current
      const revision = accepted?.cardId === active.card_id ? Math.max(active.review_revision, accepted.revision) : active.review_revision
      const edited = acceptedContent.current
      const contentRevision = edited?.cardId === active.card_id ? edited.revision : active.content_revision
      if (state.review_revision > revision && state.exists && !state.suppressed && state.content_revision === contentRevision) {
        discardReviewedCard()
      } else if (!state.exists || state.suppressed || state.review_revision !== revision || state.content_revision !== contentRevision) {
        allowCurrentCandidate.current = true
        cancelPrefetch()
        refillTask.current?.request()
        setStaleReason('This card changed while you were away. Continue to load a fresh card.')
      }
    })
    validationTask.current = task
    const unsubscribe = subscribeLiveEvents(handleLiveEvent)
    return () => { unsubscribe(); task.dispose(); validationTask.current = null }
  }, [isAuthenticated])

  useEffect(() => {
    if (!isAuthenticated) return
    const task = createRefreshTask(async current => {
      const active = currentCard.current
      if (!active || prefetchRef.current) return
      const slot = startPrefetch(allowCurrentCandidate.current ? undefined : active.card_id)
      const result = await slot.promise
      if (!current()) return
      if (!result && currentCard.current === active) {
        if (prefetchRef.current === slot) prefetchRef.current = null
        throw new Error('Successor refresh failed')
      }
    })
    refillTask.current = task
    return () => { task.dispose(); refillTask.current = null }
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
  const startPrefetch = (currentCardId?: number): PrefetchSlot => {
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
          excludeCardIds: currentCardId === undefined ? [] : [currentCardId],
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
  const loadCardCold = (): Promise<boolean> => {
    const request = fetchCardCold()
    coldLoadRef.current = request
    return request
  }

  const fetchCardCold = async (): Promise<boolean> => {
    const requestId = ++cardRequestId.current
    cancelPrefetch()
    setLoading(true)
    setError(null)
    try {
      const envelope = await getNextCard()
      if (requestId !== cardRequestId.current) return true
      if (envelope.card) {
        setStaleReason(null)
        currentCard.current = envelope.card
        setCard(envelope.card)
        setNoCards(false)
        startPrefetch(envelope.card.card_id)
      } else {
        currentCard.current = null
        setCard(null)
        setNoCards(true)
        setNextDueAt(envelope.next_due_at ?? null)
      }
      return true
    } catch (err) {
      if (requestId !== cardRequestId.current) return true
      if (err instanceof ApiError) {
        setError(err.message)
      } else {
        setError('Failed to load card')
      }
      console.error('Error fetching card:', err)
      return false
    } finally {
      if (requestId === cardRequestId.current) setLoading(false)
    }
  }

  // Advance to the next card. Uses the prefetched card if ready;
  // otherwise fetches fresh. Never sets loading=true during a
  // card-to-card transition — the old card stays visible until
  // the new one arrives, so there's no flash.
  const advanceToNextCard = async () => {
    const requestId = ++cardRequestId.current
    setError(null)
    setNoCards(false)

    const slot = prefetchRef.current

    // Fast path: prefetch already resolved with a card.
    if (slot?.result?.card) {
      const nextCard = slot.result.card
      prefetchRef.current = null
      setStaleReason(null)
      currentCard.current = nextCard
      setCard(nextCard)
      setNoCards(false)
      setLoading(false)
      startPrefetch(nextCard.card_id)
      return
    }

    // Consume an in-flight refill rather than issue a duplicate request. If
    // invalidated while awaiting it, use the replacement slot or read fresh.
    try {
      let envelope: NextCardEnvelope | null = null
      while (prefetchRef.current && requestId === cardRequestId.current) {
        const pending = prefetchRef.current
        const result = pending.result ?? await pending.promise
        if (prefetchRef.current !== pending) continue
        prefetchRef.current = null
        if (result?.card) envelope = result
        // Exclusion-based prefetch reserves one new-card allowance. An empty
        // result therefore isn't proof the normal next-card request is empty.
        break
      }
      if (requestId !== cardRequestId.current) return
      envelope ??= await getNextCard()
      if (requestId !== cardRequestId.current) return
      if (envelope.card) {
        setStaleReason(null)
        currentCard.current = envelope.card
        setCard(envelope.card)
        setNoCards(false)
        startPrefetch(envelope.card.card_id)
      } else {
        currentCard.current = null
        setCard(null)
        setNoCards(true)
        setNextDueAt(envelope.next_due_at ?? null)
      }
    } catch (err) {
      if (requestId !== cardRequestId.current) return
      if (err instanceof ApiError) {
        setError(err.message)
      } else {
        setError('Failed to load next card')
      }
      console.error('Error fetching next card:', err)
      throw err
    } finally {
      if (requestId === cardRequestId.current) setLoading(false)
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
  // never arrived. Returns null after recovering from a stale review.
  const handleCheck = async (answer: string): Promise<{ correct: boolean; reveal: CardReveal } | null> => {
    if (!card) throw new Error('No card to check')
    if (staleReason) return null
    const cardId = card.card_id
    const submission = ++submissionVersion.current

    setSubmitError(null)
    setChecking(true)
    checkingRef.current = true

    try {
      const startedAt = performance.now()
      // CheckResponse is CardReveal flattened with `correct` (see
      // #[serde(flatten)] on cards::CheckResponse) - split them back apart.
      const { correct, ...reveal } = await checkAnswer(cardId, card.sentence_id, answer, card.review_revision, card.content_revision)
      if (submission !== submissionVersion.current) return null
      if (!canKeepSuccessorAfterReview(prefetchRef.current?.result?.card, cardId)) {
        cancelPrefetch()
        refillTask.current?.request()
      }
      acceptedReview.current = { cardId, revision: card.review_revision + 1 }
      setLastCheckMs(performance.now() - startedAt)
      // Refresh only after the committed response: check_answer is
      // what actually records the review server-side - firing this before
      // the request even started meant the stats fetch usually won the race
      // against its own write and rendered the accuracy from before this
      // card, correcting itself only on the next unrelated fetch (a manual
      // refresh, or the next scheduled refresh).
      setStatsKey((prev) => prev + 1)
      return { correct, reveal }
    } catch (err) {
      if (submission !== submissionVersion.current) return null
      if (err instanceof ApiError && err.status === 409 && err.code === 'stale_review') {
        cancelPrefetch()
        discardReviewedCard()
        setStatsKey(value => value + 1)
        return null
      }
      if (err instanceof ApiError && err.status === 409 && (err.code === 'stale_content' || err.code === 'stale_prompt')) {
        // The answer belongs to an older prompt. Fetch the current version;
        // no review was recorded, so there is no reveal or stats refresh.
        setStaleReason(err.code === 'stale_prompt'
          ? 'The selected sentence changed. Continue to load a fresh card.'
          : 'This card was edited. Continue to load a fresh card.')
        await loadCardCold()
        return null
      }
      if (err instanceof ApiError) {
        setSubmitError(`Failed to check answer: ${err.message}`)
      } else {
        setSubmitError('Failed to check answer')
      }
      validationTask.current?.request()
      console.error('Error checking answer:', err)
      throw err
    } finally {
      if (submission === submissionVersion.current) {
        setChecking(false)
        checkingRef.current = false
      }
    }
  }

  const handleSuppress = async () => {
    await loadCardCold()
    setStatsKey((prev) => prev + 1)
  }

  // Recheck the empty state at a deadline or when returning after it passed.
  // A card already on screen must never be swapped out from under the user.
  const handleCardsAvailable = async (): Promise<boolean> => {
    if (loading && coldLoadRef.current) return coldLoadRef.current
    if (noCards && !card) return loadCardCold()
    return true
  }

  // EditCardDialog edits the backend's canonical full-card shape (see its
  // doc comment in lib/api.ts), most of which - word, definition, sentence,
  // target, alternatives - isn't part of `card` (CardPrompt) at
  // all; that half lives only in Flashcard's local `reveal` state, which
  // patches itself directly from the same `updates` (see flashcard.tsx's own
  // onSaved wrapper). Only the fields CardPrompt actually has are relevant
  // here.
  const handleCardUpdated = (updates: Partial<AdminCard>, contentRevision: number) => {
    if (card) acceptedContent.current = { cardId: card.card_id, revision: contentRevision }
    const filtered = Object.fromEntries(
      Object.entries(updates).filter(([key, v]) => CARD_PROMPT_FIELDS.has(key) && v !== undefined)
    ) as Partial<CardPrompt>
    setCard(prev => prev ? { ...prev, ...filtered } : prev)
  }

  return (
    <>
      <AuthDialog />
      <AppHeader />
      <main
        data-card-theme={cardThemeId}
        aria-busy={loading || checking}
        className="ct-page min-h-screen flex flex-col items-center justify-center p-6"
      >
        <h1 className="sr-only">Hwaiting — Korean review</h1>
        {!isAuthenticated ? null : loading ? (
          <div role="status" className="text-center text-muted-foreground">
            <p>Loading card...</p>
          </div>
        ) : noCards && !card && !error ? (
          <div role="status" className="text-center text-muted-foreground">
            <div aria-hidden className="w-64 mx-auto mb-4">
              <DotLottieReact
                src="/Taegeukgi.json"
                loop
                autoplay
              />
            </div>
            <p className="mb-2">No cards to review right now</p>
            <NextDueCountdown dueAt={nextDueAt} />
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
              staleReason={staleReason}
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
      {isAuthenticated && (
        <div className="fixed inset-x-4 bottom-4 grid justify-items-center gap-2 lg:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] lg:items-end">
          <DebugStatusBar lastCheckMs={lastCheckMs} cardId={card?.card_id ?? null} />
          <StatusIndicator
            refreshKey={statsKey}
            nextCardDueAt={noCards ? nextDueAt : null}
            onCardsAvailable={handleCardsAvailable}
          />
        </div>
      )}
    </>
  )
}

function App() {
  return (
    <ThemeProvider>
      <CardThemeProvider>
        <AuthProvider>
          <SettingsProvider>
            <FieldValuesProvider>
              <AppContent />
            </FieldValuesProvider>
          </SettingsProvider>
        </AuthProvider>
      </CardThemeProvider>
    </ThemeProvider>
  )
}

export default App
