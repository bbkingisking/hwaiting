import { useId, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { ApiError, submitFeedback } from '@/lib/api'

interface FeedbackDialogProps {
  sentenceId: number
  sentence: string
  onOpenChange: (open: boolean) => void
}

// Mount per opening so each submission starts with a fresh draft and result.
export function FeedbackDialog({ sentenceId, sentence, onOpenChange }: FeedbackDialogProps) {
  const inputId = useId()
  const [text, setText] = useState('')
  const [sending, setSending] = useState(false)
  const pending = useRef(false)
  const [sent, setSent] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleSubmit(event: React.FormEvent) {
    event.preventDefault()
    if (pending.current || !text.trim()) return
    pending.current = true
    setSending(true)
    setError(null)
    try {
      await submitFeedback(sentenceId, text.trim())
      setSent(true)
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Could not send feedback. Please try again.')
    } finally {
      pending.current = false
      setSending(false)
    }
  }

  return (
    <Dialog open onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Send feedback</DialogTitle>
          <DialogDescription>Report a problem with this sentence or its translation.</DialogDescription>
        </DialogHeader>
        <p lang="ko" className="rounded-md bg-muted p-3 leading-relaxed">{sentence}</p>
        {sent ? (
          <>
            <p role="status">Thanks! Your feedback has been sent for review.</p>
            <Button onClick={() => onOpenChange(false)}>Done</Button>
          </>
        ) : (
          <form onSubmit={handleSubmit} className="space-y-4">
            <div className="space-y-2">
              <label htmlFor={inputId} className="font-medium">What needs checking?</label>
              <textarea id={inputId} value={text} onChange={event => setText(event.target.value)}
                required disabled={sending} rows={5} placeholder="Tell us what looks wrong…"
                className="w-full resize-y rounded-md border border-input bg-background p-3 text-base outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50" />
            </div>
            {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
            <div className="flex justify-end gap-2">
              <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
              <Button type="submit" disabled={sending || !text.trim()}>{sending ? 'Sending…' : 'Send feedback'}</Button>
            </div>
          </form>
        )}
      </DialogContent>
    </Dialog>
  )
}
