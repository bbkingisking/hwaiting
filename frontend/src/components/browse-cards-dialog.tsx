import { useState, useEffect } from 'react'
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
import { searchCardsByTarget, ApiError, type AdminCard } from '@/lib/api'

interface BrowseCardsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function BrowseCardsDialog({ open, onOpenChange }: BrowseCardsDialogProps) {
  const [query, setQuery] = useState('')
  const [cards, setCards] = useState<AdminCard[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [hasSearched, setHasSearched] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [selectedCard, setSelectedCard] = useState<AdminCard | null>(null)
  const [editOpen, setEditOpen] = useState(false)

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

    setIsLoading(true)
    const controller = new AbortController()
    const timer = setTimeout(() => {
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
              Search cards by their target and click one to edit it
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-2">
            <Input
              value={query}
              onChange={e => setQuery(e.target.value)}
              placeholder="Search by target…"
              autoFocus
            />

            {error && (
              <div className="text-sm text-destructive bg-destructive/10 p-3 rounded-md">
                {error}
              </div>
            )}

            {isLoading ? (
              <div className="flex items-center justify-center py-8">
                <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
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
                        {card.word} · {card.trans_word}
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
