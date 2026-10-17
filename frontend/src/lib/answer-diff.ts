// Mark the characters in an incorrect answer that still occur in the target in
// the same order. This uses a longest-common-subsequence alignment rather than
// comparing identical indexes, so omitted characters do not make the rest of
// the answer look wrong (for example, `면` against `고치려면`).
export function alignedAnswerCharacters(answer: string, target: string): boolean[] {
  const answerChars = Array.from(answer)
  const targetChars = Array.from(target)
  const lengths = Array.from(
    { length: answerChars.length + 1 },
    () => Array<number>(targetChars.length + 1).fill(0),
  )

  for (let answerIndex = answerChars.length - 1; answerIndex >= 0; answerIndex--) {
    for (let targetIndex = targetChars.length - 1; targetIndex >= 0; targetIndex--) {
      lengths[answerIndex][targetIndex] = answerChars[answerIndex] === targetChars[targetIndex]
        ? lengths[answerIndex + 1][targetIndex + 1] + 1
        : Math.max(lengths[answerIndex + 1][targetIndex], lengths[answerIndex][targetIndex + 1])
    }
  }

  const aligned = Array<boolean>(answerChars.length).fill(false)
  let answerIndex = 0
  let targetIndex = 0

  while (answerIndex < answerChars.length && targetIndex < targetChars.length) {
    if (answerChars[answerIndex] === targetChars[targetIndex]) {
      aligned[answerIndex] = true
      answerIndex++
      targetIndex++
    } else if (lengths[answerIndex + 1][targetIndex] > lengths[answerIndex][targetIndex + 1]) {
      answerIndex++
    } else {
      targetIndex++
    }
  }

  return aligned
}
