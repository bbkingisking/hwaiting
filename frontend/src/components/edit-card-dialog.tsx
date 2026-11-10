import { cardEditPatch, toFormState, type FormState } from '@/lib/card-edit-draft'
import { useState, useEffect, useId, useRef } from 'react'
import { editCard, ApiError, CARD_BACK_FIELDS, type AdminCard } from '@/lib/api'
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
import { Switch } from '@/components/ui/switch'

interface EditCardDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  card: AdminCard
}

// The "front"/"back" of a physical flashcard, reconstructed from AdminCard
// rather than fetched: CardPrompt (the front - what a reviewer sees before
// answering) and CardReveal (the back - what grading discloses) are only
// ever produced mid-review, by GET /api/cards/queue and POST
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

export function EditCardDialog({ open, onOpenChange, card }: EditCardDialogProps) {
  const [form, setForm] = useState<FormState>(toFormState(card))
  const [baseline, setBaseline] = useState(card)
  const wasOpen = useRef(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState<'full' | 'front' | 'back' | null>(null)
  const { speechLevel, tense, grammarPattern } = useFieldValues()

  // Reset form whenever the dialog opens with a (potentially new) card
  useEffect(() => {
    if (open && (!wasOpen.current || baseline.card_id !== card.card_id || baseline.sentence_id !== card.sentence_id)) {
      setBaseline(card)
      setForm(toFormState(card))
      setError(null)
      setCopied(null)
    }
    wasOpen.current = open
  }, [open, card, baseline])

  function handleChange(field: keyof FormState) {
    return (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>) => {
      setForm(prev => ({ ...prev, [field]: e.target.value }))
    }
  }

  function handleSwitchChange(field: 'is_honorific' | 'is_humble') {
    return (checked: boolean) => {
      setForm(prev => ({ ...prev, [field]: checked }))
    }
  }

  async function handleSave() {
    setSaving(true)
    setError(null)
    try {
      await editCard(baseline.card_id, cardEditPatch(baseline, form))
      onOpenChange(false)
    } catch (err) {
      setError(err instanceof ApiError && err.status === 409
        ? 'This card has changed since you opened it. Your edits have not been saved. Copy your changes, then close and reopen the card from refreshed results before trying again.'
        : err instanceof Error ? err.message : 'Failed to save')
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
      <DialogContent className="sm:max-w-2xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Edit card #{card.card_id}, sentence #{card.sentence_id}</DialogTitle>
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
          <Field label="Sentence ID">
            {id => <Input id={id} value={card.sentence_id} readOnly className="text-muted-foreground font-mono" />}
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

          <div className="border-t pt-3 flex flex-col gap-3">
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label="Word (Korean)">
                {id => <Input id={id} value={form.word} onChange={handleChange('word')} />}
              </Field>
              <Field label="Translation word (English)">
                {id => <Input id={id} value={form.trans_word} onChange={handleChange('trans_word')} />}
              </Field>
            </div>

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

          {/* Korean orthography is phonetic, so a separate Hanja reading is
              unnecessary: the card's Korean word supplies that reading. */}
          <div className="border-t pt-3">
            <Field label="Hanja">
              {id => <Input id={id} value={form.hanja} onChange={handleChange('hanja')} />}
            </Field>
          </div>

          <div className="border-t pt-3 flex flex-col gap-3">
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label="Tense">
                {id => <EnumSelect id={id} options={tense} value={form.tense} onChange={handleChange('tense')} />}
              </Field>
              <Field label="Politeness level">
                {id => <EnumSelect id={id} options={speechLevel} value={form.speech_level} onChange={handleChange('speech_level')} />}
              </Field>
            </div>

            <Field label="Grammar pattern">
              {id => <EnumSelect id={id} options={grammarPattern} value={form.grammar_pattern} onChange={handleChange('grammar_pattern')} />}
            </Field>

            <div className="grid gap-3 sm:grid-cols-2">
              <SwitchField label="Subject honorific" checked={form.is_honorific} onCheckedChange={handleSwitchChange('is_honorific')} />
              <SwitchField label="Object honorific" checked={form.is_humble} onCheckedChange={handleSwitchChange('is_humble')} />
            </div>
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

function SwitchField({ label, checked, onCheckedChange }: { label: string; checked: boolean; onCheckedChange: (checked: boolean) => void }) {
  const id = useId()
  return (
    <div className="flex min-h-9 items-center justify-between rounded-md border border-input bg-muted/30 px-3">
      <Label htmlFor={id} className="text-xs text-muted-foreground">{label}</Label>
      <Switch
        id={id}
        checked={checked}
        onCheckedChange={onCheckedChange}
        size="sm"
      />
    </div>
  )
}
