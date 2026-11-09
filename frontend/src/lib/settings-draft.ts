import type { Settings } from './types'

// The server's fitted-parameter flag is changed by optimize/reset actions,
// not by the ordinary settings form. Everything else can be saved together.
export function changedSettings(base: Settings, draft: Settings): Partial<Omit<Settings, 'hasFsrsParameters'>> {
  const changes: Record<string, unknown> = {}
  for (const key of Object.keys(draft) as (keyof Settings)[]) {
    if (key !== 'hasFsrsParameters' && draft[key] !== base[key]) changes[key] = draft[key]
  }
  return changes as Partial<Omit<Settings, 'hasFsrsParameters'>>
}
