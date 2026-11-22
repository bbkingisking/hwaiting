import { useEffect, useState } from 'react'
import { getAvailableTranslationLanguages } from '@/lib/api'
import { useAuth } from '@/components/auth-provider'
import { Button } from '@/components/ui/button'
import type { AvailableTranslationLanguage } from '@/lib/api-generated/types.gen'

interface Props {
  saving: boolean
  saveError: string | null
  onSelect: (language: string) => Promise<void>
}

export function TranslationLanguageOnboarding({ saving, saveError, onSelect }: Props) {
  const [languages, setLanguages] = useState<AvailableTranslationLanguage[]>([])
  const [selected, setSelected] = useState('')
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [revision, setRevision] = useState(0)
  const [signoutError, setSignoutError] = useState<string | null>(null)
  const { logout } = useAuth()
  const names = new Intl.DisplayNames('en', { type: 'language', fallback: 'none' })

  useEffect(() => {
    const controller = new AbortController()
    setLoading(true)
    setLoadError(null)
    getAvailableTranslationLanguages(controller.signal).then((values) => {
      if (!controller.signal.aborted) setLanguages(values)
    }).catch((error) => {
      if (!controller.signal.aborted) {
        setLoadError(error instanceof Error ? error.message : 'Could not load languages')
      }
    }).finally(() => {
      if (!controller.signal.aborted) setLoading(false)
    })
    return () => controller.abort()
  }, [revision])

  return (
    <main className='flex min-h-screen items-center justify-center bg-background p-6'>
      <section
        aria-labelledby='choose-language-title'
        className='w-full max-w-md rounded-xl border bg-card p-6 shadow-sm'
      >
        <p className='mb-4 text-sm font-medium text-muted-foreground'>Hwaiting</p>
        <h1 id='choose-language-title' className='text-xl font-semibold'>Choose your translation language</h1>
        <p className='mt-2 text-sm leading-relaxed text-muted-foreground'>
          Which language would you like to learn Korean through? We’ll save this to your account, so you only
          need to choose once.
        </p>
        {loading
          ? <p role='status' className='mt-6 text-sm text-muted-foreground'>Loading languages…</p>
          : loadError
          ? (
            <div className='mt-6 space-y-3'>
              <p role='alert' className='text-sm text-destructive'>{loadError}</p>
              <Button variant='outline' onClick={() => setRevision((value) => value + 1)}>Try again</Button>
            </div>
          )
          : languages.length === 0
          ? (
            <p role='status' className='mt-6 text-sm text-muted-foreground'>
              There are no translated cards available yet. Please check back later.
            </p>
          )
          : (
            <form
              className='mt-6 space-y-5'
              onSubmit={(event) => {
                event.preventDefault()
                if (selected && !saving) void onSelect(selected).catch(() => {})
              }}
            >
              <fieldset disabled={saving} className='space-y-3'>
                <legend className='sr-only'>Translation language</legend>
                {languages.map(({ code, card_count }) => (
                  <label key={code} className='relative block cursor-pointer'>
                    <input
                      type='radio'
                      name='translation-language'
                      value={code}
                      checked={selected === code}
                      onChange={() =>
                        setSelected(code)}
                      className='peer sr-only'
                    />
                    <span className='flex min-h-20 items-center justify-between gap-4 rounded-lg border border-input px-4 py-3 transition-colors hover:bg-muted/40 peer-checked:border-primary peer-checked:bg-primary/5 peer-focus-visible:ring-2 peer-focus-visible:ring-ring peer-disabled:cursor-wait peer-disabled:opacity-60'>
                      <span className='flex items-center gap-3'>
                        {code === 'eng' && (
                          <img
                            src='/flags/us.svg'
                            alt=''
                            className='h-6 w-10 rounded-sm object-cover ring-1 ring-black/10'
                          />
                        )}
                        <span className='font-medium'>{names.of(code) ?? code}</span>
                      </span>
                      <span className='text-right text-xs text-muted-foreground'>
                        <span className='block font-medium tabular-nums text-foreground'>
                          {card_count.toLocaleString('en')} {card_count === 1 ? 'card' : 'cards'}
                        </span>
                        <span>available</span>
                      </span>
                    </span>
                  </label>
                ))}
              </fieldset>
              {saveError && <p role='alert' className='text-sm text-destructive'>{saveError}</p>}
              <Button type='submit' className='w-full' disabled={!selected || saving}>
                {saving ? 'Saving…' : 'Start learning'}
              </Button>
            </form>
          )}
        {signoutError && <p role='alert' className='mt-4 text-sm text-destructive'>{signoutError}</p>}
        <button
          type='button'
          disabled={saving}
          onClick={async () => {
            const result = await logout()
            if (!result.success) setSignoutError(result.error ?? 'Could not sign out')
          }}
          className='mt-6 text-sm text-muted-foreground underline underline-offset-4 disabled:opacity-50'
        >
          Sign out
        </button>
      </section>
    </main>
  )
}
