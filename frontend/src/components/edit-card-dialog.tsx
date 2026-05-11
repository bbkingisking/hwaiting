import { useState, useEffect } from 'react'
import type { Card } from '@/lib/types'
import { editCard } from '@/lib/api'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

interface EditCardDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  card: Card
}

interface FormState {
  word: string
  definition: string
  pos: string
  origin_type: string
  hanja: string
  hanja_eum: string
  grade: string
  trans_word: string
  trans_dfn: string
  sentence: string
  sentence_translation: string
  target: string
}

function toFormState(card: Card): FormState {
  return {
    word: card.word ?? '',
    definition: card.definition ?? '',
    pos: card.pos ?? '',
    origin_type: card.origin_type ?? '',
    hanja: card.hanja ?? '',
    hanja_eum: card.hanja_eum ?? '',
    grade: card.grade ?? '',
    trans_word: card.trans_word ?? '',
    trans_dfn: card.trans_dfn ?? '',
    sentence: card.sentence ?? '',
    sentence_translation: card.sentence_translation ?? '',
    target: card.target ?? '',
  }
}

function nullIfEmpty(val: string): string | null {
  return val.trim() === '' ? null : val.trim()
}

export function EditCardDialog({ open, onOpenChange, card }: EditCardDialogProps) {
  const [form, setForm] = useState<FormState>(toFormState(card))
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Reset form whenever the dialog opens with a (potentially new) card
  useEffect(() => {
    if (open) {
      setForm(toFormState(card))
      setError(null)
    }
  }, [open, card])

  function handleChange(field: keyof FormState) {
    return (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => {
      setForm(prev => ({ ...prev, [field]: e.target.value }))
    }
  }

  async function handleSave() {
    setSaving(true)
    setError(null)
    try {
      await editCard(card.card_id, {
        word: form.word.trim() || undefined,
        definition: nullIfEmpty(form.definition),
        pos: nullIfEmpty(form.pos),
        origin_type: nullIfEmpty(form.origin_type),
        hanja: nullIfEmpty(form.hanja),
        hanja_eum: nullIfEmpty(form.hanja_eum),
        grade: nullIfEmpty(form.grade),
        trans_word: form.trans_word.trim() || undefined,
        trans_dfn: nullIfEmpty(form.trans_dfn),
        sentence: form.sentence.trim() || undefined,
        sentence_translation: form.sentence_translation.trim() || undefined,
        target: form.target.trim() || undefined,
      })
      onOpenChange(false)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save')
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Edit card</DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-3 py-1">
          <Field label="Word (Korean)">
            <Input value={form.word} onChange={handleChange('word')} />
          </Field>

          <Field label="Definition (Korean)">
            <textarea
              value={form.definition}
              onChange={handleChange('definition')}
              rows={2}
              className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring resize-none"
            />
          </Field>

          <div className="grid grid-cols-2 gap-3">
            <Field label="Part of speech">
              <Input value={form.pos} onChange={handleChange('pos')} />
            </Field>
            <Field label="Grade">
              <Input value={form.grade} onChange={handleChange('grade')} />
            </Field>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <Field label="Hanja">
              <Input value={form.hanja} onChange={handleChange('hanja')} />
            </Field>
            <Field label="Hanja reading">
              <Input value={form.hanja_eum} onChange={handleChange('hanja_eum')} />
            </Field>
          </div>

          <Field label="Origin type">
            <Input value={form.origin_type} onChange={handleChange('origin_type')} placeholder="e.g. 고유어, 한자어" />
          </Field>

          <div className="border-t pt-3 flex flex-col gap-3">
            <Field label="Translation word (English)">
              <Input value={form.trans_word} onChange={handleChange('trans_word')} />
            </Field>

            <Field label="Translation definition (English)">
              <textarea
                value={form.trans_dfn}
                onChange={handleChange('trans_dfn')}
                rows={2}
                className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring resize-none"
              />
            </Field>
          </div>

          <div className="border-t pt-3 flex flex-col gap-3">
            <Field label="Sentence (Korean)">
              <textarea
                value={form.sentence}
                onChange={handleChange('sentence')}
                rows={2}
                className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring resize-none"
              />
            </Field>

            <Field label="Target (conjugated form in sentence)">
              <Input value={form.target} onChange={handleChange('target')} />
            </Field>

            <Field label="Sentence translation (English)">
              <textarea
                value={form.sentence_translation}
                onChange={handleChange('sentence_translation')}
                rows={2}
                className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring resize-none"
              />
            </Field>
          </div>

          {error && (
            <p className="text-sm text-destructive">{error}</p>
          )}
        </div>

        <DialogFooter showCloseButton>
          <Button onClick={handleSave} disabled={saving}>
            {saving ? 'Saving…' : 'Save'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label className="text-xs text-muted-foreground">{label}</Label>
      {children}
    </div>
  )
}