import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { ApiError, listFeedback, type FeedbackEntry } from '@/lib/api'
import { EditCardDialog } from '@/components/edit-card-dialog'
import { Loader2 } from 'lucide-react'

interface CheckFeedbackDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function CheckFeedbackDialog({ open, onOpenChange }: CheckFeedbackDialogProps) {
  const [selectedCardId, setSelectedCardId] = useState<number | null>(null)
  const [editOpen, setEditOpen] = useState(false)
  const [feedback, setFeedback] = useState<FeedbackEntry[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [reload, setReload] = useState(0)

  useEffect(() => {
    if (!open) return
    const controller = new AbortController()
    setLoading(true)
    setError(null)
    listFeedback(controller.signal).then(result => {
      if (!controller.signal.aborted) setFeedback(result.feedback)
    }).catch(err => {
      if (!controller.signal.aborted) setError(err instanceof ApiError ? err.message : 'Could not load feedback. Please try again.')
    }).finally(() => {
      if (!controller.signal.aborted) setLoading(false)
    })
    return () => controller.abort()
  }, [open, reload])

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>Check feedback</DialogTitle>
            <DialogDescription>Sentence feedback from everyone, oldest first. Select an entry to edit its card.</DialogDescription>
          </DialogHeader>
          {loading ? (
            <div role="status" className="flex items-center justify-center gap-2 py-8">
              <Loader2 aria-hidden className="h-5 w-5 animate-spin" /> Loading feedback…
            </div>
          ) : error ? (
            <div className="space-y-3">
              <p role="alert" className="text-destructive">{error}</p>
              <Button variant="outline" onClick={() => setReload(value => value + 1)}>Try again</Button>
            </div>
          ) : (
            <>
              <p role="status" className="text-sm text-muted-foreground">{feedback.length} {feedback.length === 1 ? 'submission' : 'submissions'}</p>
              {feedback.length === 0 ? <p className="py-8 text-center text-muted-foreground">No feedback yet.</p> : (
                <ol className="space-y-3">
                  {feedback.map(entry => (
                    <li key={entry.id}>
                      <button type="button" data-card-id={entry.card_id}
                        onClick={() => { setSelectedCardId(entry.card_id); setEditOpen(true) }}
                        className="w-full space-y-3 rounded-lg border p-4 text-left hover:bg-accent/50 transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring">
                        <span className="sr-only">Edit card #{entry.card_id}: </span>
                        <span className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
                          <span>Sentence #{entry.sentence_id} · User #{entry.user_id}</span>
                          <time dateTime={entry.created_at}>{new Date(entry.created_at).toLocaleString()}</time>
                        </span>
                        <span className="block whitespace-pre-wrap wrap-anywhere leading-relaxed">{entry.body}</span>
                      </button>
                    </li>
                  ))}
                </ol>
              )}
            </>
          )}
        </DialogContent>
      </Dialog>
      {selectedCardId !== null && (
        <EditCardDialog open={open && editOpen} onOpenChange={setEditOpen} cardId={selectedCardId} />
      )}
    </>
  )
}
