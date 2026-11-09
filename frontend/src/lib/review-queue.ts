import type { CardPrompt } from './types'

// Replace the disposable buffer with a snapshot. A newer revision of the
// active card may be a legitimate repetition; only its displayed attempt is
// withheld. The active answer/reveal itself belongs to the component.
export function reconcileQueue(cards: CardPrompt[], active: CardPrompt | null): CardPrompt[] {
  const seen = new Set<number>()
  return cards.filter(card => {
    if (seen.has(card.card_id)) return false
    seen.add(card.card_id)
    return !active || card.card_id !== active.card_id ||
      card.review_revision !== active.review_revision ||
      card.content_revision !== active.content_revision ||
      card.sentence_id !== active.sentence_id
  })
}
