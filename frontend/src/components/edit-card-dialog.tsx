import { copyJson } from '@/lib/clipboard'
import { Check, Copy } from 'lucide-react'
import { type FormState } from '@/lib/card-edit-draft'
import { adminCardViews, initialDrafts, updateDrafts, draftChanges, viewKey } from '@/lib/admin-card-draft'
import { useEffect, useId, useState } from 'react'
import { type AdminCardDetail, ApiError, getAdminCard, saveAdminCard, getGrammarPatterns } from '@/lib/api'
import { useSettings } from '@/components/settings-provider'
import { krdictUrl } from '@/lib/utils'
import { speechLevel, tense, type DisplayOption } from '@/lib/vocabulary'
import { EnumSelect } from '@/components/enum-select'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'

interface EditCardDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  cardId: number
}

export function EditCardDialog({ open, onOpenChange, cardId }: EditCardDialogProps) {
  const [detail, setDetail] = useState<AdminCardDetail | null>(null)
  const [drafts, setDrafts] = useState<Record<string, FormState>>({})
  const [language, setLanguage] = useState('')
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const [grammarPattern, setGrammarPattern] = useState<Record<string, DisplayOption>>({})
  const { settings: { translationLanguage } } = useSettings()
  const languageNames = new Intl.DisplayNames(document.documentElement.lang || 'en', { type: 'language', fallback: 'none' })
  const views = detail ? adminCardViews(detail) : {}
  const activeKey = detail ? viewKey(detail.sentence.sentence_id, language) : ''
  const baseline = views[activeKey]
  const form = drafts[activeKey] ?? (detail ? Object.values(drafts)[0] : undefined)
  const changes = draftChanges(views, drafts)
  const dirty = changes.length > 0
  const sentence = detail?.sentence
  const languages = Object.keys(sentence?.translations ?? {}).filter(code => detail?.translations[code])

  useEffect(() => {
    if (!open) return
    const controller = new AbortController()
    setLoading(true)
    setDetail(null)
    setDrafts({})
    setError(null)
    setCopied(false)
    Promise.all([getAdminCard(cardId, controller.signal), getGrammarPatterns(controller.signal)]).then(([data, patterns]) => {
      if (controller.signal.aborted) return
      setGrammarPattern(Object.fromEntries(patterns.map(pattern => [pattern.slug, { slug: pattern.slug, label: pattern.name, endings: pattern.endings }])))
      if (controller.signal.aborted) return
      setDetail(data)
      setDrafts(initialDrafts(adminCardViews(data)))
      const codes = Object.keys(data.sentence.translations).filter(code => data.translations[code])
      setLanguage(codes.includes(translationLanguage ?? '') ? translationLanguage! : codes[0] ?? '')
    }).catch(err => {
      if (!controller.signal.aborted) setError(err instanceof Error ? err.message : 'Could not load card')
    }).finally(() => { if (!controller.signal.aborted) setLoading(false) })
    return () => controller.abort()
  }, [open, cardId])

  useEffect(() => {
    if (!open || !copied) return
    const timer = window.setTimeout(() => setCopied(false), 2000)
    return () => window.clearTimeout(timer)
  }, [open, copied])

  function setField(field: keyof FormState, value: string | boolean) {
    const key = activeKey || Object.keys(views)[0]
    if (!key) return
    setDrafts(prev => updateDrafts(prev, key, field, value))
  }

  function handleChange(field: keyof FormState) {
    return (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>) => {
      setField(field, e.target.value)
    }
  }

  function handleSwitchChange(field: 'is_honorific' | 'is_humble') {
    return (checked: boolean) => {
      setField(field, checked)
    }
  }

  async function handleSave() {
    setSaving(true)
    setError(null)
    try {
      if (!detail) return
      await saveAdminCard(cardId, { expected_content_revision: detail.content_revision, changes })
      onOpenChange(false)
    } catch (err) {
      setError(
        err instanceof ApiError && err.code === 'stale_content'
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
    await copyJson(detail)
    setCopied(true)
  }

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

  const krdictLink = krdictUrl(detail?.krdict_id)

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className='sm:max-w-2xl max-h-[90vh] overflow-y-auto'>
        <DialogHeader>
          <DialogTitle>Edit card #{cardId}{detail ? ` · ${detail.word}` : ''}</DialogTitle>
        </DialogHeader>

        {loading && <p role='status' className='py-8 text-center text-muted-foreground'>Loading card and translations…</p>}
        {detail && form && <fieldset disabled={saving} className='flex flex-col gap-6 py-2'>
          <section className='space-y-3' aria-labelledby='metadata-heading'>
            <h3 id='metadata-heading' className='text-sm font-semibold'>Metadata</h3>
            <div className='grid gap-3 sm:grid-cols-2'>
              <Field label='Card ID'>
                {(id) => (
                  <Input id={id} value={cardId} readOnly className='text-muted-foreground font-mono' />
                )}
              </Field>
              <Field label='Sentence ID'>
                {(id) => (
                  <Input
                    id={id}
                    value={detail.sentence.sentence_id}
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
                {(id) => <Input id={id} lang="ko" value={form.word} readOnly className='text-muted-foreground' />}
              </Field>
              <Field label='Hanja'>
                {(id) => <Input id={id} lang="ko" value={form.hanja} onChange={handleChange('hanja')} />}
              </Field>
            </div>
          </section>


          {baseline && <>
          <section className='space-y-3 border-t pt-5' aria-labelledby='context-heading'>
            <h3 id='context-heading' className='text-sm font-semibold'>Context</h3>
            <Field label='Sentence (Korean)'>
              {(id) => (
                <textarea
                  id={id}
                  lang="ko" value={form.sentence}
                  readOnly
                  rows={2}
                  className='text-muted-foreground w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring resize-none'
                />
              )}
            </Field>
            <div className='grid gap-3 sm:grid-cols-2'>
              <Field label='Target'>
                {(id) => <Input id={id} lang="ko" value={form.target} onChange={handleChange('target')} />}
              </Field>
              <Field label='Accepted alternatives · comma-separated'>
                {(id) => (
                  <Input
                    id={id}
                    lang="ko" value={form.alternatives}
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
                    languages.map(
                      (code) => [code, { slug: code, label: languageNames.of(code) ?? code, endings: [] }],
                    ),
                  )}
                  value={language}
                  onChange={(event) => { setLanguage(event.target.value); setCopied(false) }}
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

          </>}
        </fieldset>}
        {error && <p role='alert' className='text-sm text-destructive'>{error}</p>}

        <DialogFooter className='flex-col sm:flex-row sm:justify-between'>
          <Button variant='outline' onClick={handleCopy} disabled={loading || !detail}>
            {copied ? <Check /> : <Copy />}
            {copied ? 'Copied!' : 'Copy full card as JSON'}
          </Button>
          <div className='flex flex-col gap-2 sm:flex-row'>
            <Button variant='outline' onClick={() => handleOpenChange(false)} disabled={saving}>Close</Button>
            <Button onClick={handleSave} disabled={saving || loading || !dirty}>
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
