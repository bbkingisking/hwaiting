import { copyJson } from '@/lib/clipboard'
import { Check, Copy } from 'lucide-react'
import { cardEditPatch, type FormState, toFormState } from '@/lib/card-edit-draft'
import { useEffect, useId, useRef, useState } from 'react'
import { type AdminCard, ApiError, editCard, getSentenceTranslations } from '@/lib/api'
import { krdictUrl } from '@/lib/utils'
import { useFieldValues } from '@/components/field-values-provider'
import { EnumSelect } from '@/components/enum-select'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'

interface EditCardDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  card: AdminCard
}

export function EditCardDialog({ open, onOpenChange, card }: EditCardDialogProps) {
  const [form, setForm] = useState<FormState>(toFormState(card))
  const [baseline, setBaseline] = useState(card)
  const [translations, setTranslations] = useState<Record<string, AdminCard>>({})
  const [language, setLanguage] = useState('eng')
  const [translationLoading, setTranslationLoading] = useState(false)
  const wasOpen = useRef(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const { speechLevel, tense, grammarPattern } = useFieldValues()
  // Names follow the interface language, independently of the card's translations.
  const languageNames = new Intl.DisplayNames(document.documentElement.lang || 'en', {
    type: 'language',
    fallback: 'none',
  })

  // Reset form whenever the dialog opens with a (potentially new) card
  useEffect(() => {
    if (
      open &&
      (!wasOpen.current || baseline.card_id !== card.card_id || baseline.sentence_id !== card.sentence_id)
    ) {
      setTranslations({})
      setLanguage('eng')
      setBaseline(card)
      setForm(toFormState(card))
      setError(null)
      setCopied(false)
    }
    wasOpen.current = open
  }, [open, card, baseline])

  useEffect(() => {
    if (!open || !copied) return
    const timer = window.setTimeout(() => setCopied(false), 2000)
    return () => window.clearTimeout(timer)
  }, [open, copied])

  useEffect(() => {
    if (!open) return
    const controller = new AbortController()
    setTranslationLoading(true)
    getSentenceTranslations(card.card_id, card.sentence_id, controller.signal).then((views) => {
      if (controller.signal.aborted) return
      const all = Object.fromEntries(views.map((view) => [view.translation_language, view.card]))
      setTranslations(all)
      const selected = all.eng ? 'eng' : views[0]?.translation_language
      if (selected) {
        setLanguage(selected)
        setBaseline(all[selected])
        setForm(toFormState(all[selected]))
      }
    }).catch((err) => {
      if (!controller.signal.aborted) {
        setError(err instanceof Error ? err.message : 'Could not load translations')
      }
    }).finally(() => {
      if (!controller.signal.aborted) setTranslationLoading(false)
    })
    return () => controller.abort()
  }, [open, card.card_id, card.sentence_id])

  function selectLanguage(next: string) {
    if (next === language || !translations[next]) return
    if (
      translationDirty && !window.confirm('Discard your unsaved translation changes and switch language?')
    ) return
    const nextForm = toFormState(translations[next])
    // Korean content and grammatical properties belong to every translation.
    setForm({
      ...form,
      trans_word: nextForm.trans_word,
      trans_dfn: nextForm.trans_dfn,
      sentence_translation: nextForm.sentence_translation,
    })
    setBaseline(translations[next])
    setLanguage(next)
    setCopied(false)
  }

  function handleChange(field: keyof FormState) {
    return (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>) => {
      setForm((prev) => ({ ...prev, [field]: e.target.value }))
    }
  }

  function handleSwitchChange(field: 'is_honorific' | 'is_humble') {
    return (checked: boolean) => {
      setForm((prev) => ({ ...prev, [field]: checked }))
    }
  }

  async function handleSave() {
    setSaving(true)
    setError(null)
    try {
      await editCard(baseline.card_id, patch, language)
      onOpenChange(false)
    } catch (err) {
      setError(
        err instanceof ApiError && err.status === 409
          ? 'This card has changed since you opened it. Your edits have not been saved. Copy your changes, then close and reopen the card from refreshed results before trying again.'
          : err instanceof Error
          ? err.message
          : 'Failed to save',
      )
    } finally {
      setSaving(false)
    }
  }

  async function handleCopy() {
    await copyJson(baseline)
    setCopied(true)
  }

  const patch = cardEditPatch(baseline, form)
  const dirty = Object.keys(patch).length > 2
  const initial = toFormState(baseline)
  const translationDirty = form.trans_word !== initial.trans_word || form.trans_dfn !== initial.trans_dfn ||
    form.sentence_translation !== initial.sentence_translation

  useEffect(() => {
    if (!open || !dirty) return
    const warn = (event: BeforeUnloadEvent) => {
      event.preventDefault()
      event.returnValue = ''
    }
    window.addEventListener('beforeunload', warn)
    return () => window.removeEventListener('beforeunload', warn)
  }, [open, dirty])

  function handleOpenChange(next: boolean) {
    if (saving) return
    if (!next && dirty && !window.confirm('Discard your unsaved changes and close the editor?')) return
    onOpenChange(next)
  }

  const krdictLink = krdictUrl(card.krdict_id)

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className='sm:max-w-2xl max-h-[90vh] overflow-y-auto'>
        <DialogHeader>
          <DialogTitle>Edit card</DialogTitle>
        </DialogHeader>

        <fieldset disabled={saving || translationLoading} className='flex flex-col gap-6 py-2'>
          <section className='space-y-3' aria-labelledby='metadata-heading'>
            <h3 id='metadata-heading' className='text-sm font-semibold'>Metadata</h3>
            <div className='grid gap-3 sm:grid-cols-2'>
              <Field label='Card ID'>
                {(id) => (
                  <Input id={id} value={card.card_id} readOnly className='text-muted-foreground font-mono' />
                )}
              </Field>
              <Field label='Sentence ID'>
                {(id) => (
                  <Input
                    id={id}
                    value={card.sentence_id}
                    readOnly
                    className='text-muted-foreground font-mono'
                  />
                )}
              </Field>
            </div>
            <div className='flex flex-wrap items-center gap-x-3 gap-y-1'>
              {krdictLink && (
                <a
                  href={krdictLink}
                  target='_blank'
                  rel='noreferrer'
                  className='text-xs text-muted-foreground hover:text-foreground underline underline-offset-2 w-fit'
                >
                  Look up in KRDICT
                </a>
              )}
            </div>
          </section>

          <section className='space-y-3 border-t pt-5' aria-labelledby='dictionary-heading'>
            <h3 id='dictionary-heading' className='text-sm font-semibold'>Dictionary form</h3>
            <div className='grid gap-3 sm:grid-cols-2'>
              <Field label='Word (Korean)'>
                {(id) => <Input id={id} value={form.word} readOnly className='text-muted-foreground' />}
              </Field>
              <Field label='Hanja'>
                {(id) => <Input id={id} value={form.hanja} onChange={handleChange('hanja')} />}
              </Field>
            </div>
          </section>

          <section className='space-y-3 border-t pt-5' aria-labelledby='context-heading'>
            <h3 id='context-heading' className='text-sm font-semibold'>Context</h3>
            <Field label='Sentence (Korean)'>
              {(id) => (
                <textarea
                  id={id}
                  value={form.sentence}
                  readOnly
                  rows={2}
                  className='text-muted-foreground w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring resize-none'
                />
              )}
            </Field>
            <div className='grid gap-3 sm:grid-cols-2'>
              <Field label='Target'>
                {(id) => <Input id={id} value={form.target} onChange={handleChange('target')} />}
              </Field>
              <Field label='Accepted alternatives · comma-separated'>
                {(id) => (
                  <Input
                    id={id}
                    value={form.alternatives}
                    onChange={handleChange('alternatives')}
                    placeholder='e.g. alt1, alt2'
                  />
                )}
              </Field>
            </div>
            <div className='grid gap-3 sm:grid-cols-2'>
              <Field label='Tense'>
                {(id) => (
                  <EnumSelect id={id} options={tense} value={form.tense} onChange={handleChange('tense')} />
                )}
              </Field>
              <Field label='Politeness level'>
                {(id) => (
                  <EnumSelect
                    id={id}
                    options={speechLevel}
                    value={form.speech_level}
                    onChange={handleChange('speech_level')}
                  />
                )}
              </Field>
            </div>
            <Field label='Grammar pattern'>
              {(id) => (
                <EnumSelect
                  id={id}
                  options={grammarPattern}
                  value={form.grammar_pattern}
                  onChange={handleChange('grammar_pattern')}
                />
              )}
            </Field>
            <div className='grid gap-3 sm:grid-cols-2'>
              <SwitchField
                label='Subject honorific'
                checked={form.is_honorific}
                onCheckedChange={handleSwitchChange('is_honorific')}
              />
              <SwitchField
                label='Object honorific'
                checked={form.is_humble}
                onCheckedChange={handleSwitchChange('is_humble')}
              />
            </div>
          </section>

          <section className='space-y-3 border-t pt-5' aria-labelledby='translations-heading'>
            <h3 id='translations-heading' className='text-sm font-semibold'>Translations</h3>
            <Field label='Language'>
              {(id) => (
                <EnumSelect
                  id={id}
                  allowEmpty={false}
                  options={Object.fromEntries(
                    Object.keys(translations).map(
                      (code) => [code, { slug: code, label: languageNames.of(code) ?? code, endings: [] }],
                    ),
                  )}
                  value={language}
                  onChange={(event) => selectLanguage(event.target.value)}
                />
              )}
            </Field>
            <div className='rounded-lg border border-border bg-muted/15 p-4 space-y-3'>
              <Field label='Word'>
                {(id) => <Input id={id} value={form.trans_word} onChange={handleChange('trans_word')} />}
              </Field>
              <Field label='Definition'>
                {(id) => (
                  <textarea
                    id={id}
                    value={form.trans_dfn}
                    readOnly
                    rows={2}
                    className='text-muted-foreground w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring resize-none'
                  />
                )}
              </Field>
              <Field label='Sentence'>
                {(id) => (
                  <textarea
                    id={id}
                    value={form.sentence_translation}
                    onChange={handleChange('sentence_translation')}
                    rows={2}
                    className='w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring resize-none'
                  />
                )}
              </Field>
            </div>
          </section>

          {error && <p role='alert' className='text-sm text-destructive'>{error}</p>}
        </fieldset>

        <DialogFooter className='flex-col sm:flex-row sm:justify-between'>
          <Button variant='outline' onClick={handleCopy} disabled={translationLoading}>
            {copied ? <Check /> : <Copy />}
            {copied ? 'Copied!' : 'Copy full card as JSON'}
          </Button>
          <div className='flex flex-col gap-2 sm:flex-row'>
            <Button variant='outline' onClick={() => handleOpenChange(false)} disabled={saving}>Close</Button>
            <Button onClick={handleSave} disabled={saving || translationLoading || !dirty}>
              {saving ? 'Saving…' : 'Save changes'}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// `children` is a render prop so every control gets an id the <label> can point
// at — a bare <label> with no htmlFor leaves the control nameless to assistive
// tech and to anything driving the page off the accessibility tree.
function Field({ label, children }: { label: string; children: (id: string) => React.ReactNode }) {
  const id = useId()
  return (
    <div className='flex flex-col gap-1.5'>
      <Label htmlFor={id} className='text-xs text-muted-foreground'>{label}</Label>
      {children(id)}
    </div>
  )
}

function SwitchField(
  { label, checked, onCheckedChange }: {
    label: string
    checked: boolean
    onCheckedChange: (checked: boolean) => void
  },
) {
  const id = useId()
  return (
    <div className='flex min-h-9 items-center justify-between rounded-md border border-input bg-muted/30 px-3'>
      <Label htmlFor={id} className='text-xs text-muted-foreground'>{label}</Label>
      <Switch
        id={id}
        checked={checked}
        onCheckedChange={onCheckedChange}
        size='sm'
      />
    </div>
  )
}
