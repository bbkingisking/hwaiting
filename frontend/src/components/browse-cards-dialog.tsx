import { useEffect, useRef, useState } from 'react'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { EditCardDialog } from '@/components/edit-card-dialog'
import { Loader2 } from 'lucide-react'
import { searchCardsByTarget, subscribeCardEdits, type AdminCard } from '@/lib/api'
import { useDebouncedCardSearch } from '@/hooks/use-debounced-card-search'

interface BrowseCardsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function BrowseCardsDialog({ open, onOpenChange }: BrowseCardsDialogProps) {
  const { query, setQuery, cards, setCards, isLoading, hasSearched, error, setError } = useDebouncedCardSearch(open)
  const [selectedCard, setSelectedCard] = useState<AdminCard | null>(null)
  const [editOpen, setEditOpen] = useState(false)
  const currentQuery = useRef(query)
  currentQuery.current = query

  const handleCardClick = (card: AdminCard) => {
    setSelectedCard(card)
    setEditOpen(true)
  }

  useEffect(() => {
    if (!open) return
    return subscribeCardEdits(({ card }) => {
      setSelectedCard(prev => prev?.sentence_id === card.sentence_id ? card : prev)
      // Card-wide fields affect every sentence and a changed target may no
      // longer match this search. Replace the edited row immediately, then
      // read the server's full canonical result set.
      setCards(prev => prev.map(row => row.sentence_id === card.sentence_id ? card : row))
      const searched = currentQuery.current.trim()
      if (!searched) return
      void searchCardsByTarget(searched).then(result => {
        if (currentQuery.current.trim() === searched) setCards(result.cards)
      }).catch(err => {
        if (currentQuery.current.trim() === searched) {
          setError(err instanceof Error ? err.message : 'Could not refresh cards')
        }
      })
    })
  }, [open, setCards, setError])

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="sm:max-w-lg max-h-[90vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Browse Cards</DialogTitle>
            <DialogDescription>
              Search cards by their target or card id and click one to edit it
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
              <div role="alert" className="text-sm text-destructive bg-destructive/10 p-3 rounded-md">
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
                <div role="status" className="text-sm text-muted-foreground text-center py-8">
                  No cards found
                </div>
              )
            ) : (
              <ul aria-label="Matching card sentences" className="space-y-2 max-h-96 overflow-y-auto">
                {cards.map(card => (
                  <li key={card.sentence_id}>
                    <button
                      type="button"
                      data-card-id={card.card_id}
                      data-sentence-id={card.sentence_id}
                      onClick={() => handleCardClick(card)}
                      className="w-full text-left p-3 border rounded-md hover:bg-accent/50 transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    >
                      <span className="sr-only">Edit </span>
                      <span className="block space-y-1">
                        <span className="block font-medium">{card.target}</span>
                        <span className="block text-sm text-muted-foreground">
                          {card.word} · {card.trans_word} · card #{card.card_id} · sentence #{card.sentence_id}
                        </span>
                        <span className="block text-xs text-muted-foreground/70">
                          {card.sentence}
                        </span>
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </DialogContent>
      </Dialog>

      {selectedCard && (
        <EditCardDialog
          open={editOpen}
          onOpenChange={setEditOpen}
          card={selectedCard}
        />
      )}
    </>
  )
}
