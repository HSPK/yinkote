/**
 * Following the tail of a conversation.
 *
 * The rule looks obvious until paging exists: a list that got longer has not
 * necessarily got longer *at the end*.
 */
import { describe, expect, it } from 'vitest'

import { shouldFollow } from './follow'

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
