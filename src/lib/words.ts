export interface Word {
  form: string
  hint: string
  context: string
  contextTranslation: string
  grammar: string | null
  politeness: string | null
  notes: string[]
}

function parseWords(raw: string): Word[] {
  const lines = raw.trim().split('\n')
  const words: Word[] = []
  for (let i = 1; i < lines.length; i++) {
    try {
      const entry = JSON.parse(lines[i])
      const h = entry.homographs?.[0]
      const sense = h?.senses?.[0]
      const translation = sense?.translations?.[0]
      const ctx = sense?.contexts?.[0]

      const form = h?.form
      const hint = translation?.translation
      const context = ctx?.context
      const contextTranslation = ctx?.translations?.[0]?.translation

      if (!form || !hint || !context || !contextTranslation) continue

      const grammar: string | null = h?.parsed_grammar?.fragments?.[0]?.full ?? null

      const comments: string[] = (translation?.comments ?? []).map((c: { comment: string }) => c.comment)
      const politenessPatterns = [
        'formal and casual speech',
        'formal and polite speech',
        'informal and casual speech',
        'informal and polite speech',
        'informal or formal situations',
      ]
      const politeness = comments.find(c =>
        politenessPatterns.some(p => c.toLowerCase().includes(p))
      ) ?? null
      const notes = comments.filter(c =>
        !politenessPatterns.some(p => c.toLowerCase().includes(p))
      )

      words.push({ form, hint, context, contextTranslation, grammar, politeness, notes })
    } catch {
      // skip malformed lines
    }
  }
  return words
}

import raw from '../../words.json?raw'
export const words = parseWords(raw)

export function getRandomWord(): Word {
  return words[Math.floor(Math.random() * words.length)]
}
