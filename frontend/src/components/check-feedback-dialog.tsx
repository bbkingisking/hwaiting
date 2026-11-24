import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { ApiError, deleteFeedback, listFeedback, type FeedbackEntry } from '@/lib/api'
import { EditCardDialog } from '@/components/edit-card-dialog'
import { Loader2, Trash2 } from 'lucide-react'

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
  const [deleteError, setDeleteError] = useState<string | null>(null)
  const [deletingId, setDeletingId] = useState<number | null>(null)

  const handleDelete = async (feedbackId: number) => {
    setDeletingId(feedbackId)
    setDeleteError(null)
    try {
      await deleteFeedback(feedbackId)
      setFeedback(entries => entries.filter(entry => entry.id !== feedbackId))
    } catch (err) {
      // Another admin may have deleted it already; either way it is gone.
      if (err instanceof ApiError && err.code === 'not_found') {
        setFeedback(entries => entries.filter(entry => entry.id !== feedbackId))
      } else {
        setDeleteError(err instanceof ApiError ? err.message : 'Could not delete feedback. Please try again.')
      }
    } finally {
      setDeletingId(null)
    }
  }

  useEffect(() => {
    if (!open) return
    const controller = new AbortController()
    setLoading(true)
    setError(null)
    setDeleteError(null)
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
              {deleteError && <p role="alert" className="text-sm text-destructive">{deleteError}</p>}
              {feedback.length === 0 ? <p className="py-8 text-center text-muted-foreground">No feedback yet.</p> : (
                <ol className="space-y-3">
                  {feedback.map(entry => (
                    // Two sibling buttons in one bordered row: edit fills the
                    // width, delete is a full-height strip on the right.
                    <li key={entry.id} className="group flex overflow-hidden rounded-lg border">
                      <button type="button" data-card-id={entry.card_id}
                        onClick={() => { setSelectedCardId(entry.card_id); setEditOpen(true) }}
                        className="min-w-0 flex-1 space-y-3 p-4 text-left hover:bg-accent/50 transition-colors outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring">
                        <span className="sr-only">Edit card #{entry.card_id}: </span>
                        <span className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
                          <span>Sentence #{entry.sentence_id} · User #{entry.user_id}</span>
                          <time dateTime={entry.created_at}>{new Date(entry.created_at).toLocaleString()}</time>
                        </span>
                        <span className="block whitespace-pre-wrap wrap-anywhere leading-relaxed">{entry.body}</span>
                      </button>
                      <button
                        type="button"
                        aria-label={`Delete feedback #${entry.id}`}
                        disabled={deletingId === entry.id}
                        onClick={() => handleDelete(entry.id)}
                        className="flex w-12 shrink-0 items-center justify-center border-l text-muted-foreground transition-colors outline-none hover:bg-destructive/10 hover:text-destructive focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring disabled:opacity-50"
                      >
                        {/* Icon hidden until hover or focus on mouse devices; always shown on touch screens. */}
                        {deletingId === entry.id
                          ? <Loader2 aria-hidden className="h-4 w-4 animate-spin" />
                          : <Trash2 aria-hidden className="h-4 w-4 transition-opacity pointer-fine:opacity-0 pointer-fine:group-hover:opacity-100 pointer-fine:group-focus-within:opacity-100" />}
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
