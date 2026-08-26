import { describe, expect, it } from 'vitest'

import { SCOPE_KEYS, applyClick, captureScope, emptyScope, rangeOf } from './scope'

const keys = ['a', 'b', 'c', 'd', 'e']
const at = (selected: string[], anchor: number) => ({ selected, anchor })

describe('scope', () => {
  it('derives its key list from a real scope, so the two cannot drift', () => {
    expect(SCOPE_KEYS).toContain('items')
    expect(SCOPE_KEYS).toContain('selected')
    expect(SCOPE_KEYS).toHaveLength(Object.keys(emptyScope()).length)
  })

  it('captures exactly the fields a tab owns and nothing else', () => {
    const captured = captureScope({ ...emptyScope(), query: 'x', extra: 1 } as never)
    expect(Object.keys(captured).sort()).toEqual([...SCOPE_KEYS].sort())
    expect(captured.query).toBe('x')
  })
})

describe('rangeOf', () => {
  it('is inclusive at both ends', () => {
    expect(rangeOf(keys, 1, 3)).toEqual(['b', 'c', 'd'])
  })

  it('selects the same rows dragged up as dragged down', () => {
    expect(rangeOf(keys, 3, 1)).toEqual(rangeOf(keys, 1, 3))
  })

  it('clamps to the list rather than returning holes', () => {
    expect(rangeOf(keys, -5, 1)).toEqual(['a', 'b'])
    expect(rangeOf(keys, 3, 99)).toEqual(['d', 'e'])
  })
})

describe('applyClick', () => {
  it('replaces the selection on a plain click', () => {
    expect(applyClick(keys, at(['a', 'b'], 0), 3, 'none')).toEqual({
      selected: ['d'],
      anchor: 3,
      cursor: 3,
    })
  })

  it('adds and removes with the toggle modifier', () => {
    const added = applyClick(keys, at(['a'], 0), 2, 'toggle')
    expect(added.selected).toEqual(['a', 'c'])
    expect(applyClick(keys, at(added.selected, added.anchor), 2, 'toggle').selected).toEqual(['a'])
  })

  it('selects a range from the anchor', () => {
    expect(applyClick(keys, at(['b'], 1), 3, 'range').selected).toEqual(['b', 'c', 'd'])
  })

  it('keeps the anchor so a range can be adjusted by shift-clicking again', () => {
    // Moving the anchor would make the second shift-click select from the
    // wrong end, which is not how any file manager behaves.
    const first = applyClick(keys, at(['b'], 1), 3, 'range')
    expect(first.anchor).toBe(1)
    expect(applyClick(keys, at(first.selected, first.anchor), 4, 'range').selected).toEqual([
      'b',
      'c',
      'd',
      'e',
    ])
  })

  it('moves the anchor on a plain or toggling click', () => {
    expect(applyClick(keys, at([], 0), 2, 'none').anchor).toBe(2)
    expect(applyClick(keys, at([], 0), 2, 'toggle').anchor).toBe(2)
  })

  it('ignores a click past the end of the list', () => {
    const current = at(['a'], 0)
    expect(applyClick(keys, current, 99, 'none')).toEqual({ ...current, cursor: 99 })
  })
})
