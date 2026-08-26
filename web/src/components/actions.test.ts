import { describe, expect, it } from 'vitest'

import { asMenuItem, destroySelected, globalActions, newCollection, newItem } from './actions'

describe('actions', () => {
  it('gives every action a distinct id, since they key both surfaces', () => {
    const ids = globalActions().map((a) => a.id)
    expect(new Set(ids).size).toBe(ids.length)
  })

  it('labels every action, so neither surface has to invent one', () => {
    for (const action of globalActions()) {
      expect(action.label, action.id).toBeTruthy()
      expect(action.label, action.id).not.toMatch(/^[a-z]+\./)
    }
  })

  it('offers exactly one way to create a collection', () => {
    // It used to offer two: a bare name prompt and the full editor, so which
    // one you got depended on where you clicked.
    const creators = globalActions().filter((a) => a.id.includes('collection'))
    expect(creators).toHaveLength(1)
    expect(creators[0]?.id).toBe(newCollection().id)
  })

  it('carries the shortcut through to a menu entry', () => {
    const entry = asMenuItem(newItem())
    expect(entry.hint).toBe('N')
    expect(entry.label).toBe(newItem().label)
    expect(entry.onSelect).toBeTypeOf('function')
  })

  it('marks destructive actions so both surfaces can warn alike', () => {
    expect(destroySelected(3).danger).toBe(true)
    expect(asMenuItem(destroySelected(3)).danger).toBe(true)
  })
})
