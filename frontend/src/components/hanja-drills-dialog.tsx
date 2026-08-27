import { useState, useLayoutEffect, useRef, useCallback } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Loader2, Check } from 'lucide-react'
import { getHanjaDrill, type HanjaDrill, ApiError } from '@/lib/api'

interface HanjaDrillsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

// Not part of the review flow: a random hanja from a card the user has
// already mastered, for free-form recall practice. There's nothing to grade
// here - typing anything and submitting reveals the word/meaning, so
// `answer` and `revealed` only ever drive what's shown, never a request.
export function HanjaDrillsDialog({ open, onOpenChange }: HanjaDrillsDialogProps) {
  const [drill, setDrill] = useState<HanjaDrill | null>(null)
  const [answer, setAnswer] = useState('')
  const [revealed, setRevealed] = useState(false)
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const abortRef = useRef<AbortController | null>(null)

  const loadDrill = useCallback(() => {
    abortRef.current?.abort()
    const controller = new AbortController()
    abortRef.current = controller

    setIsLoading(true)
    setError(null)
    setAnswer('')
    setRevealed(false)

    getHanjaDrill(controller.signal)
      .then(response => {
        if (controller.signal.aborted) return
        setDrill(response.drill ?? null)
      })
      .catch(err => {
        if (controller.signal.aborted) return
        console.error('Failed to load hanja drill:', err)
        setError(err instanceof ApiError ? err.message : 'Failed to load drill')
      })
      .finally(() => {
        if (!controller.signal.aborted) setIsLoading(false)
      })
  }, [])

  // useLayoutEffect, not useEffect: the reset needs to land before the
  // browser paints the reopened dialog, or the previous drill's reveal (or a
  // blank frame, before isLoading flips true) would flash on screen for one
  // frame ahead of the fresh load - this component stays mounted between
  // opens, so its state doesn't reset on its own.
  useLayoutEffect(() => {
    if (open) {
      loadDrill()
    } else {
      abortRef.current?.abort()
    }
  }, [open, loadDrill])

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!revealed) {
      setRevealed(true)
    } else {
      loadDrill()
    }
  }

  // Cosmetic only, never sent anywhere: any answer counts, this just decides
  // whether to show a little checkmark next to the reveal.
  const pastedTheHanja = !!drill && answer.trim() === drill.hanja

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-106.25">
        <DialogHeader>
          <DialogTitle>Hanja Drills</DialogTitle>
          <DialogDescription>
            Recall the reading and meaning of hanja you've already mastered. Nothing here is graded or tracked.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {error && (
            <div className="text-sm text-destructive bg-destructive/10 p-3 rounded-md">
              {error}
            </div>
          )}

          {isLoading ? (
            <div role="status" className="flex items-center justify-center py-8">
              <Loader2 aria-hidden className="h-6 w-6 animate-spin text-muted-foreground" />
              <span className="sr-only">Loading drill…</span>
            </div>
          ) : drill === null ? (
            <div className="text-sm text-muted-foreground text-center py-8">
              No mastered hanja cards yet — keep reviewing and they'll show up here.
            </div>
          ) : (
            <form onSubmit={handleSubmit} className="space-y-4">
              <div className="text-center text-6xl py-6" lang="ko">
                {drill.hanja}
              </div>

              <Input
                value={answer}
                onChange={(e) => setAnswer(e.target.value)}
                placeholder="Type or paste the reading…"
                lang="ko"
                autoFocus
                aria-label="Answer"
              />

              {revealed && (
                <div className="rounded-md border p-3 space-y-1">
                  <div className="flex items-center gap-2 font-medium" lang="ko">
                    {drill.word}
                    {pastedTheHanja && (
                      <Check aria-label="Matched the hanja" className="h-4 w-4 text-green-600 dark:text-green-400" />
                    )}
                  </div>
                  <div className="text-sm text-muted-foreground">{drill.trans_word}</div>
                  {drill.trans_dfn && (
                    <div className="text-xs text-muted-foreground">{drill.trans_dfn}</div>
                  )}
                </div>
              )}

              <div className="flex justify-end gap-2">
                {revealed ? (
                  <Button type="submit">Next</Button>
                ) : (
                  <Button type="submit">Reveal</Button>
                )}
              </div>
            </form>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
