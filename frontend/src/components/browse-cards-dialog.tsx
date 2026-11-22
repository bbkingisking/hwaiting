import { useEffect, useState } from 'react'
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
import { subscribeCardEdits, type AdminCardSummary } from '@/lib/api'
import { useDebouncedCardSearch } from '@/hooks/use-debounced-card-search'

interface BrowseCardsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function BrowseCardsDialog({ open, onOpenChange }: BrowseCardsDialogProps) {
  const { query, setQuery, cards, refreshSearch, isLoading, hasSearched, error } = useDebouncedCardSearch(open)
  const [selectedCard, setSelectedCard] = useState<AdminCardSummary | null>(null)
  const [editOpen, setEditOpen] = useState(false)
  const handleCardClick = (card: AdminCardSummary) => {
    setSelectedCard(card)
    setEditOpen(true)
  }

  useEffect(() => {
    if (!open) return
    return subscribeCardEdits(({ card }) => {
      setSelectedCard(prev => prev?.card_id === card.card_id ? { ...prev, word: card.word, pos: card.pos } : prev)
      // Search owns the result list, including whether the edited card still
      // matches the query and any card-wide fields on sibling sentences.
      refreshSearch()
    })
  }, [open, refreshSearch])

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
              <ul aria-label="Matching cards" className="space-y-2 max-h-96 overflow-y-auto">
                {cards.map(card => (
                  <li key={card.card_id}>
                    <button
                      type="button"
                      data-card-id={card.card_id}
                      onClick={() => handleCardClick(card)}
                      className="w-full text-left p-3 border rounded-md hover:bg-accent/50 transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    >
                      <span className="sr-only">Edit </span>
                      <span className="block space-y-1">
                        <span className="block font-medium">{card.word}</span>
                        <span className="block text-sm text-muted-foreground">
                          Card #{card.card_id}
                        </span>
                        <span className="block text-xs text-muted-foreground/70">
                          {card.pos}
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
          cardId={selectedCard.card_id}
        />
      )}
    </>
  )
}
