/**
 * Following the tail of a conversation.
 *
 * The rule looks obvious until paging exists: a list that got longer has not
 * necessarily got longer *at the end*.
 */
import { describe, expect, it } from 'vitest'

import { shouldFollow, shouldScroll } from './follow'

describe('following the tail', () => {
  it('starts at the bottom, where the reading is', () => {
    expect(shouldFollow(null, { id: 'm9', steps: 0 })).toBe(true)
  })

  it('follows a new message', () => {
    expect(shouldFollow({ id: 'm9', steps: 0 }, { id: 'm10', steps: 0 })).toBe(true)
  })

  it('does not follow when earlier messages are loaded', () => {
    // The list got longer at the other end. Jumping to the bottom here throws
    // away exactly the position the reader was scrolling back to find.
    expect(shouldFollow({ id: 'm40', steps: 0 }, { id: 'm40', steps: 0 })).toBe(false)
  })

  it('follows a live turn growing in place', () => {
    // The last entry is the same one, but it got taller, so the bottom moved.
    expect(shouldFollow({ id: 'live', steps: 2 }, { id: 'live', steps: 3 })).toBe(true)
  })

  it('stays put when nothing changed', () => {
    expect(shouldFollow({ id: 'live', steps: 3 }, { id: 'live', steps: 3 })).toBe(false)
  })

  it('has nowhere to go in an empty thread', () => {
    expect(shouldFollow(null, { id: undefined, steps: 0 })).toBe(false)
  })
})

describe('shouldScroll', () => {
  const at = (index: number, token = index) => ({ index, token })

  it('honours a request once and not again when the list grows', () => {
    // The library fetches another page while the reader is half way down. The
    // request has not changed; only the row count has.
    expect(shouldScroll(at(0), null, 100)).toBe(true)
    expect(shouldScroll(at(0), 0, 200)).toBe(false)
    expect(shouldScroll(at(0), 0, 300)).toBe(false)
  })

  it('honours a genuinely new request', () => {
    expect(shouldScroll(at(42), 0, 200)).toBe(true)
  })

  it('honours a repeat command for the row it is already on', () => {
    // While an answer streams, the live turn grows in place: the bottom keeps
    // moving without changing index. A second request must not be mistaken for
    // the first one arriving twice.
    expect(shouldScroll({ index: 9, token: 7 }, 6, 10)).toBe(true)
  })

  it('waits for rows before scrolling to one', () => {
    // A restored position asks for a row that has not loaded yet; the request
    // must survive until it can be met.
    expect(shouldScroll(at(30), null, 0)).toBe(false)
    expect(shouldScroll(at(30), null, 100)).toBe(true)
  })

  it('does nothing when nothing was asked for', () => {
    expect(shouldScroll(undefined, null, 100)).toBe(false)
  })
})
