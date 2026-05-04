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
import { getPosLabel, getSpeechLevelLabel, getTenseLabel } from '@/lib/utils'

interface CustomCardsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function CustomCardsDialog({ open, onOpenChange }: CustomCardsDialogProps) {
  const [showCreateForm, setShowCreateForm] = useState(false)
  const [word, setWord] = useState('')
  const [transWord, setTransWord] = useState('')
  const [sentence, setSentence] = useState('')
  const [sentenceTranslation, setSentenceTranslation] = useState('')
  const [target, setTarget] = useState('')
  const [pos, setPos] = useState('')
  const [speechLevel, setSpeechLevel] = useState('')
  const [tense, setTense] = useState('')
  const [definition, setDefinition] = useState('')
  
  const [cards, setCards] = useState<CustomCard[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [isCreating, setIsCreating] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const resetForm = () => {
    setWord('')
    setTransWord('')
    setSentence('')
    setSentenceTranslation('')
    setTarget('')
    setPos('')
    setSpeechLevel('')
    setTense('')
    setDefinition('')
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
        word,
        trans_word: transWord,
        sentence,
        sentence_translation: sentenceTranslation,
        target,
        pos: pos || null,
        speech_level: speechLevel || null,
        tense: tense || null,
        definition: definition || null,
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
                        <div className="font-medium">{card.word}</div>
                        <div className="text-sm text-muted-foreground">{card.trans_word}</div>
                        <div className="text-xs text-muted-foreground">
                          {card.sentence}
                        </div>
                        {(card.pos || card.speech_level || card.tense) && (
                          <div className="flex gap-2 pt-1">
                            {card.pos && (
                              <span className="text-xs px-2 py-0.5 rounded-full bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300">
                                {getPosLabel(card.pos)}
                              </span>
                            )}
                            {card.speech_level && (
                              <span className="text-xs px-2 py-0.5 rounded-full bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300">
                                {getSpeechLevelLabel(card.speech_level)}
                              </span>
                            )}
                            {card.tense && (
                              <span className="text-xs px-2 py-0.5 rounded-full bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300">
                                {getTenseLabel(card.tense)}
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
                <Label htmlFor="word">
                  Korean Word/Phrase <span className="text-destructive">*</span>
                </Label>
                <Input
                  id="word"
                  value={word}
                  onChange={(e) => setWord(e.target.value)}
                  placeholder="e.g., 자주"
                  required
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="trans-word">
                  Translation <span className="text-destructive">*</span>
                </Label>
                <Input
                  id="trans-word"
                  value={transWord}
                  onChange={(e) => setTransWord(e.target.value)}
                  placeholder="e.g., often, frequently"
                  required
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="sentence">
                  Example Sentence <span className="text-destructive">*</span>
                </Label>
                <Input
                  id="sentence"
                  value={sentence}
                  onChange={(e) => setSentence(e.target.value)}
                  placeholder="e.g., 우리는 자주 만나."
                  required
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="target">
                  Target Word in Sentence <span className="text-destructive">*</span>
                </Label>
                <Input
                  id="target"
                  value={target}
                  onChange={(e) => setTarget(e.target.value)}
                  placeholder="e.g., 자주 (exact text to blank out)"
                  required
                />
                <p className="text-xs text-muted-foreground">
                  This exact text will be blanked out in the sentence
                </p>
              </div>

              <div className="space-y-2">
                <Label htmlFor="sentence-translation">
                  Sentence Translation <span className="text-destructive">*</span>
                </Label>
                <Input
                  id="sentence-translation"
                  value={sentenceTranslation}
                  onChange={(e) => setSentenceTranslation(e.target.value)}
                  placeholder="e.g., We meet often."
                  required
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="pos">
                  Part of Speech
                </Label>
                <Input
                  id="pos"
                  value={pos}
                  onChange={(e) => setPos(e.target.value)}
                  placeholder="e.g., adverb, verb, noun"
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="speech-level">
                  Speech Level
                </Label>
                <Input
                  id="speech-level"
                  value={speechLevel}
                  onChange={(e) => setSpeechLevel(e.target.value)}
                  placeholder="e.g., informal polite, formal"
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="tense">
                  Tense
                </Label>
                <Input
                  id="tense"
                  value={tense}
                  onChange={(e) => setTense(e.target.value)}
                  placeholder="e.g., past, present, future"
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="definition">
                  Definition/Notes
                </Label>
                <textarea
                  id="definition"
                  value={definition}
                  onChange={(e) => setDefinition(e.target.value)}
                  placeholder="Additional definition or notes about the word"
                  className="w-full min-h-20 px-3 py-2 text-sm rounded-md border border-input bg-background ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
                  rows={3}
                />
              </div>

              <div className="flex gap-2 pt-4">
                <Button
                  onClick={handleCreateCard}
                  disabled={!word || !transWord || !sentence || !sentenceTranslation || !target || isCreating}
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