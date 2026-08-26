/** Finding text in a rendered document.
 *
 *  The matching itself is pure and tested here; the DOM work is a thin layer
 *  above it, because "which occurrences are there" is the part that goes wrong
 *  and the part worth pinning down.
 */

/** Where a needle occurs in a haystack, as [start, end) character offsets. */
export function occurrences(haystack: string, needle: string): [number, number][] {
  const query = needle.trim().toLowerCase()
  if (!query) return []

  const text = haystack.toLowerCase()
  const out: [number, number][] = []
  let from = 0
  for (;;) {
    const at = text.indexOf(query, from)
    if (at < 0) return out
    out.push([at, at + query.length])
    // Advance past this match: overlapping hits would double-count "aa" in
    // "aaa", and a reader stepping through them would never leave the word.
    from = at + query.length
  }
}

/** Move through matches, wrapping at both ends so `next` always goes somewhere. */
export function step(current: number, total: number, delta: number): number {
  if (total <= 0) return 0
  return (current + delta + total) % total
}
