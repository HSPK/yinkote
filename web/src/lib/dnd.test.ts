import { beforeEach, describe, expect, it } from 'vitest'

import { accepts, beginDrag, dragging, endDrag, readDrop } from './dnd'

/** Minimal stand-in for DataTransfer; jsdom does not construct one. */
function transfer() {
  const data = new Map<string, string>()
  return {
    effectAllowed: 'none',
    dropEffect: 'none',
    setData: (type: string, value: string) => void data.set(type, value),
    getData: (type: string) => data.get(type) ?? '',
  }
}

const event = (t = transfer()) => ({ dataTransfer: t }) as unknown as React.DragEvent

describe('dnd', () => {
  beforeEach(endDrag)

  it('reports nothing outside a drag', () => {
    expect(dragging()).toBeNull()
    expect(accepts('items')).toBeNull()
  })

  it('exposes the payload while dragging and clears it afterwards', () => {
    beginDrag(event(), { kind: 'items', keys: ['A', 'B'] }, '2 items')
    expect(accepts('items')?.keys).toEqual(['A', 'B'])
    expect(accepts('collection')).toBeNull()
    endDrag()
    expect(dragging()).toBeNull()
  })

  it('marks a collection drag as a move so the cursor is honest', () => {
    const t = transfer()
    beginDrag(event(t), { kind: 'collection', key: 'C1' }, 'Papers')
    expect(t.effectAllowed).toBe('move')
    expect(t.getData('text/plain')).toBe('Papers')
  })

  it('prefers the event payload, which survives crossing frames', () => {
    const t = transfer()
    beginDrag(event(t), { kind: 'items', keys: ['A'] }, 'one')
    endDrag()
    expect(readDrop(event(t))).toEqual({ kind: 'items', keys: ['A'] })
  })

  it('falls back to module state when the event carries nothing', () => {
    beginDrag(event(), { kind: 'items', keys: ['Z'] }, 'one')
    expect(readDrop(event())).toEqual({ kind: 'items', keys: ['Z'] })
  })

  it('survives a corrupt payload rather than throwing mid-drop', () => {
    const t = transfer()
    t.setData('application/x-yinkote', '{not json')
    expect(readDrop(event(t))).toBeNull()
  })
})
