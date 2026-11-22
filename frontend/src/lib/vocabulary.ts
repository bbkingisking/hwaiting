// Presentation text for fixed semantic values. These labels are client-owned;
// the database remains responsible for valid values and their relationships.
export interface DisplayOption {
  slug: string
  label: string
  endings?: string[]
}
function options(entries: [string, string][]): Record<string, DisplayOption> {
  return Object.fromEntries(entries.map(([slug, label]) => [slug, { slug, label }]))
}
export const pos = options([
  ['verb', 'Verb'], ['noun', 'Noun'], ['adjective', 'Adjective'], ['adverb', 'Adverb'],
  ['bound-noun', 'Bound Noun'], ['pronoun', 'Pronoun'], ['numeral', 'Numeral'],
  ['interjection', 'Interjection'], ['determiner', 'Determiner'],
  ['auxiliary-adjective', 'Auxiliary Adjective'], ['auxiliary-verb', 'Auxiliary Verb'],
  ['particle', 'Particle'], ['no-pos', 'No POS'],
])
export const originType = options([
  ['native-korean', 'Native Korean'], ['sino-korean', 'Sino-Korean'],
  ['loanword', 'Loanword'], ['hybrid', 'Hybrid'],
])
export const grade = options([['beginner', 'Beginner'], ['intermediate', 'Intermediate'], ['advanced', 'Advanced']])
export const speechLevel = options([
  ['hae-che', 'Intimate'], ['haeyo-che', 'Polite Informal'], ['hasipsio-che', 'Formal'],
  ['haera-che', 'Plain'], ['hao-che', 'Semi-Formal'], ['hage-che', 'Semi-Plain'],
])
export const tense = options([['present', 'Present'], ['past', 'Past'], ['future', 'Future']])
