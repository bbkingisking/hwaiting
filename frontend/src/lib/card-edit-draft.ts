import type { AdminCard } from './api'
import type { UpdateCardRequest } from './api-generated'

export interface FormState {
  word: string
  hanja: string
  trans_word: string
  trans_dfn: string
  sentence: string
  sentence_translation: string
  target: string
  alternatives: string
  speech_level: string
  tense: string
  is_honorific: boolean
  is_humble: boolean
  grammar_pattern: string
}

export function toFormState(card: AdminCard): FormState {
  return {
    word: card.word ?? '',
    hanja: card.hanja ?? '',
    trans_word: card.trans_word ?? '',
    trans_dfn: card.trans_dfn ?? '',
    sentence: card.sentence ?? '',
    sentence_translation: card.sentence_translation ?? '',
    target: card.target ?? '',
    alternatives: (card.alternatives ?? []).join(', '),
    speech_level: card.speech_level ?? '',
    tense: card.tense ?? '',
    is_honorific: card.is_honorific ?? false,
    is_humble: card.is_humble ?? false,
    grammar_pattern: card.grammar_pattern ?? '',
  }
}

function nullIfEmpty(val: string): string | null {
  return val.trim() === '' ? null : val.trim()
}

// Compare against the snapshot opened by this editor, never a refreshed prop.
export function cardEditPatch(baseline: AdminCard, form: FormState): UpdateCardRequest {
  const updates = {
    word: form.word.trim() || undefined,
    hanja: nullIfEmpty(form.hanja),
    trans_word: form.trans_word.trim() || undefined,
    trans_dfn: nullIfEmpty(form.trans_dfn),
    sentence: form.sentence.trim() || undefined,
    sentence_translation: form.sentence_translation.trim() || undefined,
    target: form.target.trim() || undefined,
    alternatives: form.alternatives.split(',').map(s => s.trim()).filter(s => s.length > 0),
    speech_level: nullIfEmpty(form.speech_level),
    tense: nullIfEmpty(form.tense),
    is_honorific: form.is_honorific,
    is_humble: form.is_humble,
    grammar_pattern: nullIfEmpty(form.grammar_pattern),
  }
  const initial = toFormState(baseline)
  const changed = Object.fromEntries(
    Object.entries(updates).filter(([key]) => form[key as keyof FormState] !== initial[key as keyof FormState]),
  )
  return {
    ...changed,
    sentence_id: baseline.sentence_id,
    expected_content_revision: baseline.content_revision,
  }
}
