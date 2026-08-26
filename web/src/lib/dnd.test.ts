import { beforeEach, describe, expect, it } from 'vitest'

import {
  accepts,
  beginDrag,
  dragging,
  dropZone,
  endDrag,
  readDrop,
  type DragPayload,
} from './dnd'

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

describe('dropZone', () => {
  function zone(overrides: Partial<Parameters<typeof dropZone>[0]> = {}) {
    const dropped: DragPayload[] = []
    const active: (string | null)[] = []
    const handlers = dropZone({
      id: 'collection',
      active: null,
      setActive: (id) => active.push(id),
      accepts: (p) => p.kind === 'items',
      onDrop: (p) => void dropped.push(p),
      ...overrides,
    })
    return { handlers, dropped, active }
  }

  const dragEvent = (t = transfer()) => {
    let defaultPrevented = false
    return {
      event: {
        dataTransfer: t,
        preventDefault: () => {
          defaultPrevented = true
        },
      } as unknown as React.DragEvent,
      prevented: () => defaultPrevented,
    }
  }

  beforeEach(endDrag)

  it('accepts a drop and receives the payload', () => {
    const { handlers, dropped } = zone()
    const t = transfer()
    beginDrag(event(t), { kind: 'items', keys: ['A'] }, '1 item')
    handlers.onDrop(dragEvent(t).event)
    expect(dropped).toEqual([{ kind: 'items', keys: ['A'] }])
  })

  it('still has the payload after the drag has been torn down', () => {
    // The regression this guards: clearing the drag before dispatching left
    // the handler with nothing, so the highlight worked and the drop did not.
    const { handlers, dropped } = zone()
    const t = transfer()
    beginDrag(event(t), { kind: 'items', keys: ['A'] }, '1 item')
    endDrag()
    handlers.onDrop(dragEvent(t).event)
    expect(dropped).toHaveLength(1)
  })

  it('marks itself a valid target only for payloads it wants', () => {
    const { handlers } = zone()
    beginDrag(event(), { kind: 'items', keys: ['A'] }, '1 item')
    const wanted = dragEvent()
    handlers.onDragOver(wanted.event)
    expect(wanted.prevented()).toBe(true)

    endDrag()
    beginDrag(event(), { kind: 'collection', key: 'C1' }, 'Papers')
    const unwanted = dragEvent()
    handlers.onDragOver(unwanted.event)
    expect(unwanted.prevented()).toBe(false)
  })

  it('ignores a drop it never wanted', () => {
    const { handlers, dropped } = zone()
    const t = transfer()
    beginDrag(event(t), { kind: 'collection', key: 'C1' }, 'Papers')
    handlers.onDrop(dragEvent(t).event)
    expect(dropped).toEqual([])
  })

  it('highlights while hovering and clears on the way out', () => {
    const { handlers, active } = zone()
    beginDrag(event(), { kind: 'items', keys: ['A'] }, '1 item')
    handlers.onDragOver(dragEvent().event)
    handlers.onDragLeave()
    expect(active).toEqual(['collection', null])
  })

  it('does not re-highlight the zone that is already highlighted', () => {
    const { handlers, active } = zone({ active: 'collection' })
    beginDrag(event(), { kind: 'items', keys: ['A'] }, '1 item')
    handlers.onDragOver(dragEvent().event)
    expect(active).toEqual([])
    expect(handlers['data-drop']).toBe(true)
  })
})
