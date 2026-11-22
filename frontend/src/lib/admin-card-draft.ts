import type { AdminCard, AdminCardDetail } from './api'
import { cardEditPatch, toFormState, type FormState } from './card-edit-draft'

export const viewKey = (sentenceId: number, language: string) => `${sentenceId}:${language}`

// Editing views are derived locally from one complete authoring snapshot.
export function adminCardViews(detail: AdminCardDetail): Record<string, AdminCard> {
  const views: Record<string, AdminCard> = {}
  const sentence = detail.sentence
  for (const [language, translation] of Object.entries(sentence.translations)) {
    const word = detail.translations[language]
    if (!word) continue
    const before = sentence.text.indexOf(sentence.target.form)
    views[viewKey(sentence.sentence_id, language)] = {
      card_id: detail.card_id, content_revision: detail.content_revision,
      word: detail.word, definition: detail.definition, krdict_id: detail.krdict_id,
      hanja: detail.hanja, pos: detail.pos, origin_type: detail.origin_type, grade: detail.grade,
      sentence_id: sentence.sentence_id, sentence: sentence.text,
      sentence_before: before < 0 ? sentence.text : sentence.text.slice(0, before),
      sentence_after: before < 0 ? '' : sentence.text.slice(before + sentence.target.form.length),
      target: sentence.target.form, alternatives: sentence.target.alternatives,
      speech_level: sentence.target.speech_level, tense: sentence.target.tense,
      is_honorific: sentence.target.is_honorific, is_humble: sentence.target.is_humble,
      grammar_pattern: sentence.target.grammar_pattern,
      trans_word: word.word, trans_dfn: word.definition, sentence_translation: translation,
    }
  }
  return views
}

export function initialDrafts(views: Record<string, AdminCard>): Record<string, FormState> {
  return Object.fromEntries(Object.entries(views).map(([key, card]) => [key, toFormState(card)]))
}

export function updateDrafts(drafts: Record<string, FormState>, activeKey: string, field: keyof FormState, value: string | boolean): Record<string, FormState> {
  const [sentenceId, language] = activeKey.split(':')
  return Object.fromEntries(Object.entries(drafts).map(([key, draft]) => {
    const [sid, lang] = key.split(':')
    const applies = field === 'word' || field === 'hanja' ||
      ((field === 'trans_word' || field === 'trans_dfn') ? lang === language :
        field === 'sentence_translation' ? key === activeKey : sid === sentenceId)
    return [key, applies ? { ...draft, [field]: value } : draft]
  }))
}

export function draftChanges(views: Record<string, AdminCard>, drafts: Record<string, FormState>) {
  const sharedFields = new Set(['word', 'hanja', 'sentence', 'target', 'alternatives',
    'speech_level', 'tense', 'is_honorific', 'is_humble', 'grammar_pattern'])
  const included = new Set<string>()
  return Object.entries(views).flatMap(([key, baseline]) => {
    const patch = cardEditPatch(baseline, drafts[key])
    const { expected_content_revision: _revision, ...fields } = patch
    const scoped = Object.fromEntries(Object.entries(fields).filter(([field]) => {
      if (!sharedFields.has(field)) return true
      if (included.has(field)) return false
      included.add(field)
      return true
    }))
    return Object.keys(scoped).length > 1 ? [{ ...scoped, sentence_id: baseline.sentence_id, translation_language: key.split(':')[1] }] : []
  })
}
