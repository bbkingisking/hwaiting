import type { CardPrompt } from './types'

// Replace the disposable buffer with a snapshot. A different attempt of the
// active card may be a legitimate repetition; only its displayed attempt is
// withheld. The active answer/reveal itself belongs to the component.
export function reconcileQueue(cards: CardPrompt[], active: CardPrompt | null): CardPrompt[] {
  const seen = new Set<number>()
  return cards.filter(card => {
    if (seen.has(card.card_id)) return false
    seen.add(card.card_id)
    return !active || card.card_id !== active.card_id ||
      card.attempt_token !== active.attempt_token
  })
}
