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

/** Where a list has been asked to scroll.
 *
 *  Two callers want two different things from the same list, and the
 *  difference is not decoration:
 *
 *  - A *position* — "keep the keyboard cursor in view". Asking twice for the
 *    same row means nothing new; it is already there.
 *  - A *command* — "go to the bottom, now". Asking twice is a second request,
 *    even when it names the row it named before.
 *
 *  A bare row number can only express the first, and the chat log needed the
 *  second: while an answer streams, the live turn grows in place, so the row to
 *  go to keeps its index and the bottom keeps moving. `shouldFollow` worked out
 *  that the log should follow and the request was then dropped for looking
 *  identical — the rule was right and nothing happened.
 */
export interface ScrollRequest {
  index: number
  /** Tells one request from the next when both name the same row. */
  token: number
}

/** Whether a scroll request should be acted on now.
 *
 *  `scrollTo` is a request, not a description of where the list currently is.
 *  The difference shows the moment a list loads more rows: the request has not
 *  changed, but the row count has, and an effect that watches the count will
 *  re-scroll on every page loaded.
 *
 *  That is what made the library jump to the top each time it fetched another
 *  page. The table asks for the keyboard cursor, and for anyone who has not
 *  used the keyboard the cursor is row zero, so every append scrolled the
 *  reader back to the first row of a list they were scrolling down.
 *
 *  A request still has to survive arriving before the rows do — restoring a
 *  position on first load asks for a row that does not exist yet — so an
 *  unhonoured request is held until there is something to scroll to.
 */
export function shouldScroll(
  request: ScrollRequest | undefined,
  honoured: number | null,
  rowCount: number,
): boolean {
  if (request === undefined || rowCount === 0) return false
  // Already done for this request; the list merely got longer.
  return honoured !== request.token
}
