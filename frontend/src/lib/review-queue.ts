import type { ReviewQueueCard } from './types'

// Replace the disposable buffer with a snapshot. A different attempt of the
// active card may be a legitimate repetition; only its displayed attempt is
// withheld. The active answer/reveal itself belongs to the component.
export function reconcileQueue(cards: ReviewQueueCard[], active: ReviewQueueCard | null): ReviewQueueCard[] {
  const seen = new Set<number>()
  return cards.filter(card => {
    if (seen.has(card.card.card_id)) return false
    seen.add(card.card.card_id)
    return !active || card.card.card_id !== active.card.card_id ||
      card.review.attempt_token !== active.review.attempt_token
  })
}

// Assistance can refresh while validation is in flight. Attempt identity comes
// from the signed token, not the identity of its current JavaScript object.
export function sameReviewAttempt(current: ReviewQueueCard | null, issued: ReviewQueueCard): boolean {
  return current?.review.attempt_token === issued.review.attempt_token
}
