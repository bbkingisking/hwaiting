import { useState, useEffect } from 'react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Plus, Trash2, Loader2 } from 'lucide-react'
import { 
  createCustomCard, 
  listCustomCards, 
  deleteCustomCard,
  type CustomCard,
  ApiError
} from '@/lib/api'

interface CustomCardsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function CustomCardsDialog({ open, onOpenChange }: CustomCardsDialogProps) {
  const [showCreateForm, setShowCreateForm] = useState(false)
  const [form, setForm] = useState('')
  const [hint, setHint] = useState('')
  const [context, setContext] = useState('')
  const [contextTranslation, setContextTranslation] = useState('')
  const [grammar, setGrammar] = useState('')
  const [politeness, setPoliteness] = useState('')
  const [notes, setNotes] = useState('')
  
  const [cards, setCards] = useState<CustomCard[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [isCreating, setIsCreating] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const resetForm = () => {
    setForm('')
    setHint('')
    setContext('')
    setContextTranslation('')
    setGrammar('')
    setPoliteness('')
    setNotes('')
  }

  const loadCards = async () => {
    setIsLoading(true)
    setError(null)
    try {
      const response = await listCustomCards()
      setCards(response.cards)
    } catch (err) {
      console.error('Failed to load custom cards:', err)
      setError(err instanceof ApiError ? err.message : 'Failed to load cards')
    } finally {
      setIsLoading(false)
    }
  }

  const handleCreateCard = async () => {
    setIsCreating(true)
    setError(null)
    try {
      await createCustomCard({
        form,
        hint,
        context,
        context_translation: contextTranslation,
        grammar: grammar || null,
        politeness: politeness || null,
        notes: notes ? notes.split('\n').filter(n => n.trim()) : [],
      })
      resetForm()
      setShowCreateForm(false)
      await loadCards()
    } catch (err) {
      console.error('Failed to create custom card:', err)
      setError(err instanceof ApiError ? err.message : 'Failed to create card')
    } finally {
      setIsCreating(false)
    }
  }

  const handleDeleteCard = async (cardId: number) => {
    if (!confirm('Are you sure you want to delete this card?')) {
      return
    }
    
    try {
      await deleteCustomCard(cardId)
      await loadCards()
    } catch (err) {
      console.error('Failed to delete custom card:', err)
      setError(err instanceof ApiError ? err.message : 'Failed to delete card')
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
          <DialogTitle>Custom Cards</DialogTitle>
          <DialogDescription>
            Create and manage your own flashcards
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {error && (
            <div className="text-sm text-destructive bg-destructive/10 p-3 rounded-md">
              {error}
            </div>
          )}

          {!showCreateForm ? (
            <div className="space-y-4">
              <Button onClick={() => setShowCreateForm(true)} className="w-full">
                <Plus className="mr-2 h-4 w-4" />
                Create a new card
              </Button>

              {isLoading ? (
                <div className="flex items-center justify-center py-8">
                  <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
                </div>
              ) : cards.length === 0 ? (
                <div className="text-sm text-muted-foreground text-center py-8">
                  No custom cards yet. Create your first one!
                </div>
              ) : (
                <div className="space-y-2 max-h-96 overflow-y-auto">
                  {cards.map((card) => (
                    <div
                      key={card.id}
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
                        onClick={() => handleDeleteCard(card.id)}
                        className="text-destructive hover:text-destructive"
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          ) : (
            <div className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="form">
                  Word/Phrase <span className="text-destructive">*</span>
                </Label>
                <Input
                  id="form"
                  value={form}
                  onChange={(e) => setForm(e.target.value)}
                  placeholder="e.g., 자주"
                  required
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="hint">
                  Translation/Hint <span className="text-destructive">*</span>
                </Label>
                <Input
                  id="hint"
                  value={hint}
                  onChange={(e) => setHint(e.target.value)}
                  placeholder="e.g., often, frequently"
                  required
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="context">
                  Example Sentence <span className="text-destructive">*</span>
                </Label>
                <Input
                  id="context"
                  value={context}
                  onChange={(e) => setContext(e.target.value)}
                  placeholder="e.g., 우리는 자주 만나."
                  required
                />
                <p className="text-xs text-muted-foreground">
                  The sentence should contain the word/phrase
                </p>
              </div>

              <div className="space-y-2">
                <Label htmlFor="context-translation">
                  Sentence Translation <span className="text-destructive">*</span>
                </Label>
                <Input
                  id="context-translation"
                  value={contextTranslation}
                  onChange={(e) => setContextTranslation(e.target.value)}
                  placeholder="e.g., We meet often."
                  required
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="grammar">
                  Grammar/Part of Speech
                </Label>
                <Input
                  id="grammar"
                  value={grammar}
                  onChange={(e) => setGrammar(e.target.value)}
                  placeholder="e.g., adverb, verb past tense, noun"
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="politeness">
                  Politeness Level
                </Label>
                <Input
                  id="politeness"
                  value={politeness}
                  onChange={(e) => setPoliteness(e.target.value)}
                  placeholder="e.g., informal and polite speech"
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="notes">
                  Notes
                </Label>
                <textarea
                  id="notes"
                  value={notes}
                  onChange={(e) => setNotes(e.target.value)}
                  placeholder="Additional notes (one per line)"
                  className="w-full min-h-20 px-3 py-2 text-sm rounded-md border border-input bg-background ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
                  rows={3}
                />
                <p className="text-xs text-muted-foreground">
                  Enter each note on a separate line
                </p>
              </div>

              <div className="flex gap-2 pt-4">
                <Button
                  onClick={handleCreateCard}
                  disabled={!form || !hint || !context || !contextTranslation || isCreating}
                  className="flex-1"
                >
                  {isCreating ? (
                    <>
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      Creating...
                    </>
                  ) : (
                    'Create Card'
                  )}
                </Button>
                <Button
                  onClick={() => {
                    setShowCreateForm(false)
                    resetForm()
                    setError(null)
                  }}
                  variant="outline"
                  className="flex-1"
                  disabled={isCreating}
                >
                  Cancel
                </Button>
              </div>
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}