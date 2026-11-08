// A review cannot invalidate another already-due review card. A new card is
// different: the review may consume the last daily new-card allowance, so its
// eligibility must be selected again by the backend before we use it.
export function canKeepSuccessorAfterReview(
  successor: { card_id: number; difficulty?: number | null } | null | undefined,
  reviewedCardId: number,
): boolean {
  return !!successor && successor.card_id !== reviewedCardId && successor.difficulty != null
}
