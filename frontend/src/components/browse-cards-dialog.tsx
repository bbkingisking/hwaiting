import { useState } from 'react'
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
import { type AdminCard } from '@/lib/api'
import { useDebouncedCardSearch } from '@/hooks/use-debounced-card-search'

interface BrowseCardsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function BrowseCardsDialog({ open, onOpenChange }: BrowseCardsDialogProps) {
  const { query, setQuery, cards, setCards, isLoading, hasSearched, error } = useDebouncedCardSearch(open)
  const [selectedCard, setSelectedCard] = useState<AdminCard | null>(null)
  const [editOpen, setEditOpen] = useState(false)

  const handleCardClick = (card: AdminCard) => {
    setSelectedCard(card)
    setEditOpen(true)
  }

  const handleSaved = (updates: Partial<AdminCard>) => {
    if (!selectedCard) return
    const applied = Object.fromEntries(
      Object.entries(updates).filter(([, value]) => value !== undefined)
    )
    const merged = { ...selectedCard, ...applied }
    setSelectedCard(merged)
    setCards(prev => prev.map(c => (c.card_id === merged.card_id ? merged : c)))
  }

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
                    className="w-full text-left p-3 border rounded-md hover:bg-accent/50 transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring"
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

      {selectedCard && (
        <EditCardDialog
          open={editOpen}
          onOpenChange={setEditOpen}
          card={selectedCard}
          onSaved={handleSaved}
        />
      )}
    </>
  )
}
