import { useState, useEffect, useRef } from 'react'
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
import { getCardInflections, ApiError, type AdminCard, type CardInflection } from '@/lib/api'
import { useDebouncedCardSearch } from '@/hooks/use-debounced-card-search'

interface ConjugationTablesDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

// Same search UX as BrowseCardsDialog, but clicking a result opens a
// read-only InflectionsDialog (the conjugation matrix) instead of
// EditCardDialog.
export function ConjugationTablesDialog({ open, onOpenChange }: ConjugationTablesDialogProps) {
  const { query, setQuery, cards, isLoading, hasSearched, error, setError } = useDebouncedCardSearch(open)
  const [selectedCard, setSelectedCard] = useState<AdminCard | null>(null)
  const [inflections, setInflections] = useState<CardInflection[]>([])
  const [inflectionsOpen, setInflectionsOpen] = useState(false)
  const [inflectionsLoading, setInflectionsLoading] = useState(false)
  const inflectionsAbortRef = useRef<AbortController | null>(null)

  // Cancel any in-flight inflections fetch when the dialog closes, so a late
  // response can't pop InflectionsDialog open after the admin has already
  // moved on - it stays mounted (gated on `selectedCard`, not on this
  // dialog's own `open`), so nothing else would stop that from happening.
  useEffect(() => {
    if (!open) {
      inflectionsAbortRef.current?.abort()
    }
  }, [open])

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

    inflectionsAbortRef.current?.abort()
    const controller = new AbortController()
    inflectionsAbortRef.current = controller

    getCardInflections(card.card_id, controller.signal)
      .then(response => {
        if (controller.signal.aborted) return
        setInflections(response.inflections)
        setInflectionsOpen(true)
      })
      .catch(err => {
        if (controller.signal.aborted) return
        setError(err instanceof ApiError ? err.message : 'Failed to load conjugation table')
      })
      .finally(() => {
        if (!controller.signal.aborted) setInflectionsLoading(false)
      })
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
