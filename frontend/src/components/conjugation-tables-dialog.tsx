import { useState, useEffect } from 'react'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { InflectionsDialog } from '@/components/inflections-dialog'
import { Loader2 } from 'lucide-react'
import { searchCardsByTarget, getCardInflections, ApiError, type AdminCard, type CardInflection } from '@/lib/api'

interface ConjugationTablesDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

// Same search UX as BrowseCardsDialog, but clicking a result opens a
// read-only InflectionsDialog (the conjugation matrix) instead of
// EditCardDialog.
export function ConjugationTablesDialog({ open, onOpenChange }: ConjugationTablesDialogProps) {
  const [query, setQuery] = useState('')
  const [cards, setCards] = useState<AdminCard[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [hasSearched, setHasSearched] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [selectedCard, setSelectedCard] = useState<AdminCard | null>(null)
  const [inflections, setInflections] = useState<CardInflection[]>([])
  const [inflectionsOpen, setInflectionsOpen] = useState(false)
  const [inflectionsLoading, setInflectionsLoading] = useState(false)

  // Reset state when the dialog opens
  useEffect(() => {
    if (open) {
      setQuery('')
      setCards([])
      setError(null)
      setHasSearched(false)
    }
  }, [open])

  // Debounced search on query change
  useEffect(() => {
    if (!open) return

    const trimmed = query.trim()
    if (trimmed === '') {
      setCards([])
      setHasSearched(false)
      setIsLoading(false)
      return
    }

    // Set inside the timer, not before it: flagging "loading" during the
    // debounce window advertises a request that hasn't been made yet.
    const controller = new AbortController()
    const timer = setTimeout(() => {
      setIsLoading(true)
      searchCardsByTarget(trimmed, controller.signal)
        .then(response => {
          setCards(response.cards)
          setHasSearched(true)
          setError(null)
          setIsLoading(false)
        })
        .catch(err => {
          if (controller.signal.aborted) return
          setError(err instanceof ApiError ? err.message : 'Search failed')
          setIsLoading(false)
        })
    }, 300)

    return () => {
      clearTimeout(timer)
      controller.abort()
    }
  }, [open, query])

  const handleCardClick = (card: AdminCard) => {
    // InflectionsDialog groups rows by `restricted_to_pos`, which needs a
    // real pos slug to filter against (see its groupByCategory) - the same
    // gate flashcard.tsx applies before ever mounting that dialog.
    if (!card.pos) {
      setError(`Card #${card.card_id} has no part of speech set - can't resolve its conjugation table`)
      return
    }

    setError(null)
    setSelectedCard(card)
    setInflectionsLoading(true)
    getCardInflections(card.card_id)
      .then(response => {
        setInflections(response.inflections)
        setInflectionsOpen(true)
      })
      .catch(err => {
        setError(err instanceof ApiError ? err.message : 'Failed to load conjugation table')
      })
      .finally(() => setInflectionsLoading(false))
  }

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="sm:max-w-lg max-h-[90vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Conjugation Tables</DialogTitle>
            <DialogDescription>
              Search cards by their target or card id and click one to see its conjugation table
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-2">
            <Input
              value={query}
              onChange={e => setQuery(e.target.value)}
              aria-label="Search cards by target or card id"
              placeholder="Search by target or card id…"
              autoFocus
            />

            {error && (
              <div className="text-sm text-destructive bg-destructive/10 p-3 rounded-md">
                {error}
              </div>
            )}

            {isLoading ? (
              <div role="status" className="flex items-center justify-center py-8">
                <Loader2 aria-hidden className="h-6 w-6 animate-spin text-muted-foreground" />
                <span className="sr-only">Searching cards…</span>
              </div>
            ) : cards.length === 0 ? (
              hasSearched && (
                <div className="text-sm text-muted-foreground text-center py-8">
                  No cards found
                </div>
              )
            ) : (
              <div className="space-y-2 max-h-96 overflow-y-auto">
                {cards.map(card => (
                  <button
                    key={card.card_id}
                    type="button"
                    onClick={() => handleCardClick(card)}
                    disabled={inflectionsLoading}
                    className="w-full text-left p-3 border rounded-md hover:bg-accent/50 transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
                  >
                    <div className="space-y-1">
                      <div className="font-medium">{card.target}</div>
                      <div className="text-sm text-muted-foreground">
                        {card.word} · {card.trans_word} · #{card.card_id}
                      </div>
                      <div className="text-xs text-muted-foreground/70">
                        {card.sentence}
                      </div>
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>
        </DialogContent>
      </Dialog>

      {selectedCard && selectedCard.pos && (
        <InflectionsDialog
          open={inflectionsOpen}
          onOpenChange={setInflectionsOpen}
          word={selectedCard.word}
          pos={selectedCard.pos}
          inflections={inflections}
        />
      )}
    </>
  )
}
