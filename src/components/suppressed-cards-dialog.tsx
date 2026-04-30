import { useState, useEffect } from 'react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Trash2, Loader2 } from 'lucide-react'
import { 
  listSuppressedCards,
  unsuppressCard,
  type SuppressedCard,
  ApiError
} from '@/lib/api'

interface SuppressedCardsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function SuppressedCardsDialog({ open, onOpenChange }: SuppressedCardsDialogProps) {
  const [cards, setCards] = useState<SuppressedCard[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const loadCards = async () => {
    setIsLoading(true)
    setError(null)
    try {
      const response = await listSuppressedCards()
      setCards(response.cards)
    } catch (err) {
      console.error('Failed to load suppressed cards:', err)
      setError(err instanceof ApiError ? err.message : 'Failed to load cards')
    } finally {
      setIsLoading(false)
    }
  }

  const handleUnsuppressCard = async (wordId: number) => {
    try {
      await unsuppressCard(wordId)
      await loadCards()
    } catch (err) {
      console.error('Failed to unsuppress card:', err)
      setError(err instanceof ApiError ? err.message : 'Failed to unsuppress card')
    }
  }

  useEffect(() => {
    if (open) {
      loadCards()
    }
  }, [open])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-106.25 max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Suppressed Cards</DialogTitle>
          <DialogDescription>
            Cards you've hidden from your review queue
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
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
            <div className="text-sm text-muted-foreground text-center py-8">
              No suppressed cards
            </div>
          ) : (
            <div className="space-y-2 max-h-96 overflow-y-auto">
              {cards.map((card) => (
                <div
                  key={card.word_id}
                  className="flex items-start justify-between p-3 border rounded-md hover:bg-accent/50 transition-colors"
                >
                  <div className="flex-1 space-y-1">
                    <div className="font-medium">{card.form}</div>
                    <div className="text-sm text-muted-foreground">{card.hint}</div>
                    <div className="text-xs text-muted-foreground">
                      {card.context}
                    </div>
                    {(card.grammar || card.politeness) && (
                      <div className="flex gap-2 pt-1">
                        {card.grammar && (
                          <span className="text-xs px-2 py-0.5 rounded-full bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300">
                            {card.grammar}
                          </span>
                        )}
                        {card.politeness && (
                          <span className="text-xs px-2 py-0.5 rounded-full bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300">
                            {card.politeness}
                          </span>
                        )}
                      </div>
                    )}
                  </div>
                  <Button
                    size="icon"
                    variant="ghost"
                    onClick={() => handleUnsuppressCard(card.word_id)}
                    className="text-destructive hover:text-destructive"
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}