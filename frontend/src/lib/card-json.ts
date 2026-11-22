import type { Card as AdminCard, CardFront, CardBack } from './api-generated/types.gen'

// Project review data onto the canonical card fields so copying and editing
// use the same shapes as Browse cards, without attempt or hint metadata.
function toCardFront(card: CardFront): CardFront {
  return {
    card_id: card.card_id,
    content_revision: card.content_revision,
    sentence_id: card.sentence_id,
    krdict_id: card.krdict_id,
    pos: card.pos,
    origin_type: card.origin_type,
    hanja: card.hanja,
    grade: card.grade,
    trans_word: card.trans_word,
    trans_dfn: card.trans_dfn,
    sentence_before: card.sentence_before,
    sentence_after: card.sentence_after,
    sentence_translation: card.sentence_translation,
    speech_level: card.speech_level,
    tense: card.tense,
    is_honorific: card.is_honorific,
    is_humble: card.is_humble,
    grammar_pattern: card.grammar_pattern,
  }
}

export function toAdminCard(card: CardFront, reveal: CardBack): AdminCard {
  return {
    ...toCardFront(card),
    word: reveal.word,
    definition: reveal.definition,
    sentence: reveal.sentence,
    target: reveal.target,
    alternatives: reveal.alternatives,
  }
}
