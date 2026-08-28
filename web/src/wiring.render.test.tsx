/**
 * Wiring that a pure-function test cannot reach.
 *
 * Each of these is a place where the helper was already correct and the bug was
 * in how it was called — which is exactly the shape of the two defects reported
 * against this workbench.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Collection, Item } from './api/types'
import { libraryTab, tabId } from './lib/tabs'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

/** Never resolves, so bootstrap cannot overwrite the state a test set up. A
 *  mock that resolves with a placeholder is worse than one that does not
 *  answer: it replaces real data with a shape nothing expects. */
vi.mock('./api/client', async () => (await import('./test/idleApi')).idleClient())

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

const collection = (key: string, name: string) =>
  ({ key, libraryId: 1, name, sortIndex: 0, version: 1, itemCount: 0 }) as Collection

let container: HTMLElement
let root: Root

const render = async () => {
  await act(async () => {
    root.render(<App />)
  })
}

const click = async (el: Element | null | undefined) => {
  expect(el, 'element to click').toBeTruthy()
  await act(async () => {
    el!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }))
    el!.dispatchEvent(new MouseEvent('click', { bubbles: true, button: 0 }))
  })
}

beforeEach(() => {
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: ['A', 'B'].map(item), total: 2 }),
    ready: true,
    tabs: [libraryTab('Library')],
    activeTab: 'library',
    scopes: {},
    collections: [collection('C1', 'Papers')],
    smartCollections: [],
    tags: [{ name: 'survey', count: 3, type: 0 }],
    badgeDefs: [],
  })
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

describe('the column picker', () => {
  const open = async () => {
    await render()
    await click(container.querySelector('.statusbar .column-anchor .icon-btn'))
    return container.querySelector('.column-pop')
  }

  it('opens from the status bar', async () => {
    expect(await open(), 'the picker').toBeTruthy()
  })

  it('turns two columns on without turning the first back off', async () => {
    // The reported bug: the menu captured the order when it opened, so the
    // second choice was made against a list that predated the first.
    await open()
    const before = useStore.getState().columnOrder
    const off = [...container.querySelectorAll('.column-toggle')].filter(
      (b) => !(b as HTMLElement).dataset.checked,
    )
    expect(off.length, 'something to turn on').toBeGreaterThan(1)

    await click(off[0])
    await click(container.querySelectorAll('.column-toggle')[
      [...container.querySelectorAll('.column-toggle')].findIndex(
        (b) => b.textContent === off[1]!.textContent,
      )
    ])

    const after = useStore.getState().columnOrder
    expect(after.length, `${before.length} -> ${after.length}`).toBe(before.length + 2)
  })

  it('can turn a column off again', async () => {
    await open()
    const on = [...container.querySelectorAll('.column-toggle')].filter(
      (b) => (b as HTMLElement).dataset.checked,
    )
    const before = useStore.getState().columnOrder.length
    await click(on[on.length - 1])
    expect(useStore.getState().columnOrder.length).toBe(before - 1)
  })
})

describe('closing tabs', () => {
  it('falls back to the neighbour, and offers every tab a close button', async () => {
    act(() => {
      useStore.getState().openReader('A')
      useStore.getState().keepTab(tabId('reader', 'A'))
      useStore.getState().openReader('B')
      useStore.getState().keepTab(tabId('reader', 'B'))
    })
    await render()

    const closers = container.querySelectorAll('.tab-close')
    expect(closers, 'every tab offers a close button').toHaveLength(3)

    await click(closers[2])
    expect(useStore.getState().tabs.map((t) => t.id)).toEqual(['library', tabId('reader', 'A')])
  })
})

describe('the detail pane', () => {
  it('shows the collection beside a collection list, not an item', async () => {
    act(() => {
      useStore.setState({ collection: 'C1' })
      useStore.getState().openTab({ id: tabId('collections'), kind: 'collections', title: '' })
    })
    await render()
    expect(container.querySelector('.detail-pane')?.textContent).toContain('Papers')
  })
})
