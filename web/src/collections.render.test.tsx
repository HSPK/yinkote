/**
 * The collection browser, rendered.
 *
 * Written after a graph node's click turned out to do nothing: a surface that
 * is not the item table reaches for shared state, and unit tests of that state
 * say nothing about whether the surface reaches correctly.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Collection, Item, SmartCollection } from './api/types'
import { libraryTab } from './lib/tabs'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      // Reads must not resolve, or bootstrap replaces the state under test and
      // the resulting crash reads as a product bug.
      apply: () => new Promise(() => {}),
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
    ({
      width: 1200,
      height: 800,
      top: 0,
      left: 0,
      right: 1200,
      bottom: 800,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    }) as DOMRect
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 800 })
})

const collection = (key: string, name: string, itemCount: number) =>
  ({ key, libraryId: 1, name, sortIndex: 0, version: 1, itemCount }) as Collection

const smart = (key: string, name: string, query: string) =>
  ({ key, libraryId: 1, name, query, mode: 'hybrid', sortIndex: 0, version: 1 }) as SmartCollection

const item = (key: string) =>
  ({
    key,
    libraryId: 1,
    itemType: 'journalArticle',
    title: `Paper ${key}`,
    creators: [],
    tags: [],
    collections: [],
    version: 1,
    deleted: false,
    dateAdded: 0,
    dateModified: 0,
  }) as unknown as Item

let container: HTMLElement
let root: Root

beforeEach(() => {
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [item('A')], total: 1 }),
    ready: true,
    tabs: [libraryTab('Library'), { id: 'collections', kind: 'collections', title: 'Collections' }],
    activeTab: 'collections',
    scopes: {},
    collections: [collection('C1', 'Reading', 12), collection('C2', 'Archive', 3)],
    smartCollections: [smart('S1', 'Recent surveys', 'tag:survey')],
    tags: [],
    badgeDefs: [],
    detailOpen: true,
  })
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

async function render() {
  await act(async () => {
    root.render(<App />)
  })
}

const rows = () => [...container.querySelectorAll('.browser-grid.row')]
const rowNamed = (name: string) => rows().find((r) => r.textContent?.includes(name))!

describe('the collection browser', () => {
  it('lists plain and smart collections together', async () => {
    await render()

    expect(rows()).toHaveLength(3)
    expect(container.textContent).toContain('Reading')
    expect(container.textContent).toContain('Recent surveys')
  })

  it('shows the clicked collection in the detail pane', async () => {
    await render()
    await act(async () => {
      rowNamed('Reading').dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })

    expect(rowNamed('Reading').getAttribute('data-selected')).toBe('true')
    // The pane belongs to the surface in front, so here it must describe a
    // collection rather than whichever item is still selected elsewhere.
    const detail = container.querySelector('.pane:last-child')
    expect(detail?.textContent).toContain('Reading')
  })

  it('shows a smart collection with the query that defines it', async () => {
    await render()
    await act(async () => {
      rowNamed('Recent surveys').dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })

    expect(container.textContent).toContain('tag:survey')
  })

  it('opens a tab only on the second gesture', async () => {
    await render()

    await act(async () => {
      rowNamed('Archive').dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    expect(useStore.getState().tabs).toHaveLength(2)

    // Browsing a list should not keep changing what is in front of the reader.
    await act(async () => {
      rowNamed('Archive').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
    })
    expect(useStore.getState().tabs.length).toBeGreaterThan(2)
  })

  it('does not change what the library tab is filtered to', async () => {
    await render()
    await act(async () => {
      rowNamed('Reading').dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })

    // Inspecting a collection here writes to the same field the library tab
    // filters by. Switching back must find the library as it was left.
    await act(async () => {
      useStore.getState().activateTab('library')
    })
    expect(useStore.getState().view).toBe('library')
    expect(useStore.getState().collection).toBeNull()
  })
})
