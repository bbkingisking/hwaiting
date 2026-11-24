import type { PartOfSpeech, OriginType, Grade, SpeechLevel, Tense } from './api-generated/types.gen'

// Presentation text for fixed semantic values. These labels are client-owned;
// backend enums define valid slugs; clients own labels and ordering.
export interface DisplayOption {
  slug: string
  label: string
  endings?: string[]
}
function options<T extends string>(labels: Record<T, string>): Record<T, DisplayOption> {
  return Object.fromEntries(Object.entries(labels).map(([slug, label]) => [slug, { slug, label }])) as Record<T, DisplayOption>
}
export const pos = options<PartOfSpeech>({
  'verb': 'Verb',
  'noun': 'Noun',
  'adjective': 'Adjective',
  'adverb': 'Adverb',
  'bound-noun': 'Bound Noun',
  'pronoun': 'Pronoun',
  'numeral': 'Numeral',
  'interjection': 'Interjection',
  'determiner': 'Determiner',
  'auxiliary-adjective': 'Auxiliary Adjective',
  'auxiliary-verb': 'Auxiliary Verb',
  'particle': 'Particle',
  'no-pos': 'No POS',
})
export const originType = options<OriginType>({
  'native-korean': 'Native Korean',
  'sino-korean': 'Sino-Korean',
  'loanword': 'Loanword',
  'hybrid': 'Hybrid',
})
export const grade = options<Grade>({
  'beginner': 'Beginner',
  'intermediate': 'Intermediate',
  'advanced': 'Advanced',
})
export const speechLevel = options<SpeechLevel>({
  'hae-che': 'Intimate',
  'haeyo-che': 'Polite Informal',
  'hasipsio-che': 'Formal',
  'haera-che': 'Plain',
  'hao-che': 'Semi-Formal',
  'hage-che': 'Semi-Plain',
})
export const tense = options<Tense>({
  'present': 'Present',
  'past': 'Past',
  'future': 'Future',
})
