/**
 * Drag and drop, driven through the DOM.
 *
 * The payload helpers are tested beside themselves, and were correct when the
 * feature was nonetheless dead: the handler cleared the drag before dispatching
 * it, so every drop silently did nothing while the hover highlight — a
 * different code path — kept working. Only driving the whole gesture catches
 * that shape of bug.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Collection, Item } from './api/types'
import { libraryTab } from './lib/tabs'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

/** Records what the workbench asked the server to do. */
const calls: string[] = []

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: () => {
        calls.push(path)
        // Reads must not resolve, or bootstrap replaces the test's state.
        return /add|trash|update|create|remove/.test(path)
          ? Promise.resolve({})
          : new Promise(() => {})
      },
    })
  return { api: build('api'), connectEvents: () => () => {} }
})

beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as never
  Element.prototype.getBoundingClientRect = () =>
    ({ width: 1200, height: 800, top: 0, left: 0, right: 1200, bottom: 800, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 800 })
})

const item = (key: string) =>
  ({ key, libraryId: 1, itemType: 'journalArticle', title: `Paper ${key}`, creators: [], tags: [], collections: [], version: 1, deleted: false, dateAdded: 0, dateModified: 0 }) as unknown as Item

let container: HTMLElement
let root: Root

/** A DataTransfer, which jsdom does not implement. */
function transfer() {
  const store = new Map<string, string>()
  return {
    effectAllowed: 'none',
    dropEffect: 'none',
    setData: (k: string, v: string) => void store.set(k, v),
    getData: (k: string) => store.get(k) ?? '',
    setDragImage: () => {},
  }
}

/** A DragEvent carrying the given transfer, configurable so one gesture can
 *  reuse it the way a browser does. */
function dragEvent(type: string, data = transfer()) {
  const event = new Event(type, { bubbles: true, cancelable: true }) as DragEvent
  Object.defineProperty(event, 'dataTransfer', { value: data, configurable: true })
  return event
}

/** Drag one element onto another, as a browser would. */
async function drag(from: Element, to: Element) {
  // One transfer for the whole gesture, as a browser does.
  const data = transfer()
  await act(async () => {
    from.dispatchEvent(dragEvent('dragstart', data))
  })
  await act(async () => {
    to.dispatchEvent(dragEvent('dragover', data))
  })
  await act(async () => {
    to.dispatchEvent(dragEvent('drop', data))
  })
}

beforeEach(() => {
  calls.length = 0
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: ['A', 'B'].map(item), total: 2 }),
    ready: true,
    tabs: [libraryTab('Library')],
    activeTab: 'library',
    scopes: {},
    collections: [
      { key: 'C1', libraryId: 1, name: 'Papers', sortIndex: 0, version: 1, itemCount: 0 } as Collection,
    ],
    smartCollections: [],
    tags: [{ name: 'survey', count: 3, type: 0 }],
    badgeDefs: [],
  })
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

const render = async () => {
  await act(async () => {
    root.render(<App />)
  })
}

const row = (i: number) => container.querySelectorAll('.row')[i]!
const navItem = (label: string) =>
  [...container.querySelectorAll('.nav-item')].find((n) => n.textContent?.includes(label))!

describe('dropping items', () => {
  it('files them into a collection', async () => {
    await render()
    await drag(row(0), navItem('Papers'))
    expect(calls).toContain('api.items.addToCollection')
  })

  it('puts them in the trash', async () => {
    await render()
    await drag(row(0), navItem('Trash') ?? navItem('回收站'))
    expect(calls.some((c) => c.includes('trash'))).toBe(true)
  })

  it('tags them', async () => {
    await render()
    const chip = [...container.querySelectorAll('.tag-chip')].find((c) =>
      c.textContent?.includes('survey'),
    )!
    await drag(row(0), chip)
    expect(calls).toContain('api.items.update')
  })
})

describe('dropping a collection', () => {
  it('nests it under another, and never inside itself', async () => {
    useStore.setState({
      collections: [
        { key: 'C1', libraryId: 1, name: 'Papers', sortIndex: 0, version: 1, itemCount: 0 } as Collection,
        { key: 'C2', libraryId: 1, name: 'Reading', sortIndex: 1, version: 1, itemCount: 0 } as Collection,
      ],
    })
    await render()

    await drag(navItem('Reading'), navItem('Papers'))
    expect(calls).toContain('api.collections.move')

    calls.length = 0
    await drag(navItem('Papers'), navItem('Papers'))
    expect(calls, 'a collection cannot contain itself').toEqual([])
  })
})
