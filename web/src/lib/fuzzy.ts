/** Subsequence matching for the command palette.
 *
 *  Extracted from the component so the ranking rules are testable and can be
 *  reused by any other picker.
 */

/** True when every character of `query` appears in `text`, in order. */
export function subsequenceMatch(query: string, text: string): boolean {
  if (!query) return true
  const q = query.toLowerCase()
  const t = text.toLowerCase()
  let i = 0
  for (const ch of t) {
    if (ch === q[i]) i++
    if (i === q.length) return true
  }
  return false
}

/**
 * Rank candidates so that exact and prefix matches float above scattered
 * subsequence hits, which is what makes a palette feel like it read your mind.
 */
export function rankMatches<T>(query: string, items: T[], label: (item: T) => string): T[] {
  if (!query) return items
  const q = query.toLowerCase()

  const score = (item: T): number => {
    const text = label(item).toLowerCase()
    if (text === q) return 0
    if (text.startsWith(q)) return 1
    if (text.includes(q)) return 2
    return subsequenceMatch(q, text) ? 3 : Number.POSITIVE_INFINITY
  }

  return items
    .map((item, index) => ({ item, index, score: score(item) }))
    .filter((e) => e.score !== Number.POSITIVE_INFINITY)
    // Stable within a tier: preserve the caller's ordering.
    .sort((a, b) => a.score - b.score || a.index - b.index)
    .map((e) => e.item)
}
