import { describe, expect, it } from 'vitest'

import { occurrences, step } from './find'

describe('occurrences', () => {
  it('finds every occurrence, case-insensitively', () => {
    expect(occurrences('Attention and attention', 'attention')).toEqual([
      [0, 9],
      [14, 23],
    ])
  })

  it('does not return overlapping matches', () => {
    // Overlapping hits would double-count, and stepping through them would
    // never leave the word.
    expect(occurrences('aaaa', 'aa')).toEqual([
      [0, 2],
      [2, 4],
    ])
  })

  it('ignores a blank needle rather than matching everywhere', () => {
    expect(occurrences('text', '   ')).toEqual([])
    expect(occurrences('text', '')).toEqual([])
  })

  it('trims the needle, since a trailing space is nearly always a typo', () => {
    expect(occurrences('diffusion', ' diffusion ')).toEqual([[0, 9]])
  })

  it('returns nothing when absent', () => {
    expect(occurrences('abc', 'z')).toEqual([])
  })
})

describe('step', () => {
  it('wraps at both ends, so next always goes somewhere', () => {
    expect(step(2, 3, 1)).toBe(0)
    expect(step(0, 3, -1)).toBe(2)
  })

  it('stays at zero when there is nothing to step through', () => {
    expect(step(0, 0, 1)).toBe(0)
  })
})
