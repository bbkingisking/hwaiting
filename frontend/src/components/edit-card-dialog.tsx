import { useState, useEffect, useId } from 'react'
import { editCard, CARD_BACK_FIELDS, type AdminCard } from '@/lib/api'
import { krdictUrl } from '@/lib/utils'
import { useFieldValues } from '@/components/field-values-provider'
import { EnumSelect } from '@/components/enum-select'
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
  card: AdminCard
  onSaved?: (updates: Partial<AdminCard>) => void
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
  alternatives: string
  speech_level: string
  tense: string
  grammar_pattern: string
}

function toFormState(card: AdminCard): FormState {
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
    alternatives: (card.alternatives ?? []).join(', '),
    speech_level: card.speech_level ?? '',
    tense: card.tense ?? '',
    grammar_pattern: card.grammar_pattern ?? '',
  }
}

function nullIfEmpty(val: string): string | null {
  return val.trim() === '' ? null : val.trim()
}

// The "front"/"back" of a physical flashcard, reconstructed from AdminCard
// rather than fetched: CardPrompt (the front - what a reviewer sees before
// answering) and CardReveal (the back - what grading discloses) are only
// ever produced mid-review, by GET /api/cards/next and POST
// /api/cards/{id}/check respectively (see toAdminCard in flashcard.tsx,
// which merges the two the other direction). EditCardDialog can be opened
// on any searched card with no review in flight (browse-cards-dialog.tsx),
// so there's no live request that would return either shape here - the
// split has to be computed from the AdminCard already in hand, using the
// same CARD_BACK_FIELDS partition toAdminCard itself was built from.
function splitCard(card: AdminCard): { front: Record<string, unknown>; back: Record<string, unknown> } {
  const front: Record<string, unknown> = {}
  const back: Record<string, unknown> = {}
  for (const [key, value] of Object.entries(card)) {
    const bucket = CARD_BACK_FIELDS.has(key) ? back : front
    bucket[key] = value
  }
  return { front, back }
}

async function copyText(text: string) {
  // navigator.clipboard requires a secure context (https or localhost); this
  // app is also served over plain http on the LAN, so fall back to the
  // legacy execCommand copy path when the async API isn't available.
  if (navigator.clipboard) {
    await navigator.clipboard.writeText(text)
  } else {
    const textarea = document.createElement('textarea')
    textarea.value = text
    textarea.style.position = 'fixed'
    textarea.style.opacity = '0'
    document.body.appendChild(textarea)
    textarea.select()
    document.execCommand('copy')
    document.body.removeChild(textarea)
  }
}

export function EditCardDialog({ open, onOpenChange, card, onSaved }: EditCardDialogProps) {
  const [form, setForm] = useState<FormState>(toFormState(card))
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState<'full' | 'front' | 'back' | null>(null)
  const { pos, originType, grade, speechLevel, tense, grammarPattern } = useFieldValues()

  // Reset form whenever the dialog opens with a (potentially new) card
  useEffect(() => {
    if (open) {
      setForm(toFormState(card))
      setError(null)
      setCopied(null)
    }
  }, [open, card])

  function handleChange(field: keyof FormState) {
    return (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>) => {
      setForm(prev => ({ ...prev, [field]: e.target.value }))
    }
  }

  async function handleSave() {
    setSaving(true)
    setError(null)
    try {
      const updates = {
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
        alternatives: form.alternatives.split(',').map(s => s.trim()).filter(s => s.length > 0),
        speech_level: nullIfEmpty(form.speech_level),
        tense: nullIfEmpty(form.tense),
        grammar_pattern: nullIfEmpty(form.grammar_pattern),
      }
      await editCard(card.card_id, updates)
      onSaved?.(updates)
      onOpenChange(false)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save')
    } finally {
      setSaving(false)
    }
  }

  const { front: cardFront, back: cardBack } = splitCard(card)

  async function handleCopy(which: 'full' | 'front' | 'back') {
    const value = which === 'full' ? card : which === 'front' ? cardFront : cardBack
    await copyText(JSON.stringify(value, null, 2))
    setCopied(which)
  }

  const krdictLink = krdictUrl(card.krdict_id)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Edit card</DialogTitle>
          {krdictLink && (
            <a
              href={krdictLink}
              target="_blank"
              rel="noreferrer"
              className="text-xs text-muted-foreground hover:text-foreground underline underline-offset-2 w-fit"
            >
              Look up in KRDICT
            </a>
          )}
          <div className="flex flex-wrap gap-x-3 gap-y-1">
            <CopyLink label="Copy card as JSON" copied={copied === 'full'} onClick={() => handleCopy('full')} />
            <CopyLink
              label="Copy card_front as JSON"
              title="The hint half: what a reviewer sees before answering (CardPrompt)."
              copied={copied === 'front'}
              onClick={() => handleCopy('front')}
            />
            <CopyLink
              label="Copy card_back as JSON"
              title="The secret half: what grading discloses (CardReveal)."
              copied={copied === 'back'}
              onClick={() => handleCopy('back')}
            />
          </div>
        </DialogHeader>

        <div className="flex flex-col gap-3 py-1">
          <Field label="Card ID">
            {id => <Input id={id} value={card.card_id} readOnly className="text-muted-foreground font-mono" />}
          </Field>

          <Field label="Word (Korean)">
            {id => <Input id={id} value={form.word} onChange={handleChange('word')} />}
          </Field>

          <Field label="Definition (Korean)">
            {id => (
              <textarea
                id={id}
                value={form.definition}
                onChange={handleChange('definition')}
                rows={2}
                className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring resize-none"
              />
            )}
          </Field>

          <div className="grid grid-cols-2 gap-3">
            <Field label="Part of speech">
              {id => <EnumSelect id={id} options={pos} value={form.pos} onChange={handleChange('pos')} />}
            </Field>
            <Field label="Grade">
              {id => <EnumSelect id={id} options={grade} value={form.grade} onChange={handleChange('grade')} />}
            </Field>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <Field label="Hanja">
              {id => <Input id={id} value={form.hanja} onChange={handleChange('hanja')} />}
            </Field>
            <Field label="Hanja reading">
              {id => <Input id={id} value={form.hanja_eum} onChange={handleChange('hanja_eum')} />}
            </Field>
          </div>

          <Field label="Origin type">
            {id => <EnumSelect id={id} options={originType} value={form.origin_type} onChange={handleChange('origin_type')} />}
          </Field>

          <div className="grid grid-cols-2 gap-3">
            <Field label="Politeness level">
              {id => <EnumSelect id={id} options={speechLevel} value={form.speech_level} onChange={handleChange('speech_level')} />}
            </Field>
            <Field label="Tense">
              {id => <EnumSelect id={id} options={tense} value={form.tense} onChange={handleChange('tense')} />}
            </Field>
          </div>

          <Field label="Grammar pattern">
            {id => <EnumSelect id={id} options={grammarPattern} value={form.grammar_pattern} onChange={handleChange('grammar_pattern')} />}
          </Field>

          <div className="border-t pt-3 flex flex-col gap-3">
            <Field label="Translation word (English)">
              {id => <Input id={id} value={form.trans_word} onChange={handleChange('trans_word')} />}
            </Field>

            <Field label="Translation definition (English)">
              {id => (
                <textarea
                  id={id}
                  value={form.trans_dfn}
                  onChange={handleChange('trans_dfn')}
                  rows={2}
                  className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring resize-none"
                />
              )}
            </Field>
          </div>

          <div className="border-t pt-3 flex flex-col gap-3">
            <Field label="Sentence (Korean)">
              {id => (
                <textarea
                  id={id}
                  value={form.sentence}
                  onChange={handleChange('sentence')}
                  rows={2}
                  className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring resize-none"
                />
              )}
            </Field>

            <Field label="Target (conjugated form in sentence)">
              {id => <Input id={id} value={form.target} onChange={handleChange('target')} />}
            </Field>

            <Field label="Accepted alternatives (comma-separated)">
              {id => (
                <Input
                  id={id}
                  value={form.alternatives}
                  onChange={handleChange('alternatives')}
                  placeholder="e.g. alt1, alt2"
                />
              )}
            </Field>

            <Field label="Sentence translation (English)">
              {id => (
                <textarea
                  id={id}
                  value={form.sentence_translation}
                  onChange={handleChange('sentence_translation')}
                  rows={2}
                  className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring resize-none"
                />
              )}
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

function CopyLink({ label, title, copied, onClick }: { label: string; title?: string; copied: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className="text-xs text-muted-foreground hover:text-foreground underline underline-offset-2 w-fit"
    >
      {copied ? 'Copied!' : label}
    </button>
  )
}

// `children` is a render prop so every control gets an id the <label> can point
// at — a bare <label> with no htmlFor leaves the control nameless to assistive
// tech and to anything driving the page off the accessibility tree.
function Field({ label, children }: { label: string; children: (id: string) => React.ReactNode }) {
  const id = useId()
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={id} className="text-xs text-muted-foreground">{label}</Label>
      {children(id)}
    </div>
  )
}
