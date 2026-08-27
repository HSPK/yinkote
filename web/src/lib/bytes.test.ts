/**
 * Sizes as a person reads them.
 *
 * The number this replaces was `319459328`, printed in a toast.
 */
import { describe, expect, it } from 'vitest'

import { humanBytes } from './maintenance'

describe('humanBytes', () => {
  it('keeps small numbers exact and large ones readable', () => {
    expect(humanBytes(0)).toBe('0 B')
    expect(humanBytes(999)).toBe('999 B')
    expect(humanBytes(319_459_328)).toBe('305 MB')
  })

  it('gives a decimal only where it carries information', () => {
    // 1.5 GB says something 2 GB does not; 305.4 MB says nothing 305 MB does.
    expect(humanBytes(1_610_612_736)).toBe('1.5 GB')
    expect(humanBytes(1024)).toBe('1.0 KB')
  })

  it('stops at a unit it can name', () => {
    expect(humanBytes(2 ** 60)).toContain('TB')
  })
})
