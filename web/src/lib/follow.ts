/** Whether the log should jump to the bottom.
 *
 *  A conversation is read from the bottom, so a message that lands off screen
 *  looks like nothing happened — the log has to follow the tail.
 *
 *  But "the list got longer" is not the same as "something new arrived".
 *  Loading earlier messages makes it longer at the *other* end, and following
 *  the count there yanks the reader to the bottom of a thread they were
 *  deliberately scrolling back through.
 *
 *  So the tail is identified by what is at it, not by how many things precede
 *  it.
 */
export interface Tail {
  /** Identity of the last entry, or undefined when there is none. */
  id?: string
  /** How many steps the live turn has taken, since that grows in place. */
  steps: number
}

export function shouldFollow(previous: Tail | null, next: Tail): boolean {
  if (!next.id) return false
  // First view of a thread: start at the bottom, where the reading is.
  if (!previous) return true
  if (previous.id !== next.id) return true
  // Same last entry, but the live turn grew — its own height changed, so the
  // bottom moved even though nothing was added.
  return next.steps > previous.steps
}
