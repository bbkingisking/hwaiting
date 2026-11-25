import type { CardFront, CardBack } from './api-generated/types.gen'

type MetadataKey = 'card_id' | 'content_revision' | 'sentence_id' | 'krdict_id' | 'grade' | 'origin_type'

export interface CardJson {
  metadata: Pick<CardFront, MetadataKey>
  front: Omit<CardFront, MetadataKey>
  back: CardBack
}

// Explicitly select fields to exclude review-only hint and attempt metadata.
// Editing passes the selected translation's saved view through this too.
export function toCardJson(card: CardFront, reveal: CardBack): CardJson {
  return {
    metadata: {
      card_id: card.card_id,
      content_revision: card.content_revision,
      sentence_id: card.sentence_id,
      krdict_id: card.krdict_id,
      grade: card.grade,
      origin_type: card.origin_type,
    },
    front: {
      pos: card.pos,
      hanja: card.hanja,
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
    },
    back: {
      word: reveal.word,
      definition: reveal.definition,
      sentence: reveal.sentence,
      target: reveal.target,
      alternatives: reveal.alternatives,
    },
  }
}
