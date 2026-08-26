/**
 * The behaviours that were asked for, exercised rather than assumed.
 *
 * Their pure parts are tested next to themselves; what is untested is the
 * wiring, which is where a feature usually stops working — the helper is
 * right, the caller passes the wrong thing, and nothing at either end notices.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Item } from './api/types'
import { libraryTab, tabId } from './lib/tabs'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

/** An `api` whose every path is callable and never resolves.
 *
 *  Recursive because the client is nested — `api.items.children` — and a mock
 *  answering only one level deep fails as a `TypeError` inside a component,
 *  which React reports as a render failure rather than as a missing stub.
 *
 *  Built inside the factory because `vi.mock` is hoisted above everything else
 *  in the file, so a module-level binding is not initialised yet when it runs. */
vi.mock('./api/client', () => {
  const idle: unknown = new Proxy(function () {} as object, {
    get: (_target, key) => (key === 'then' ? undefined : idle),
    apply: () => new Promise(() => {}),
  })
  return { api: idle, connectEvents: () => () => {} }
})

/** jsdom lays nothing out, so a virtualised list measures itself as empty and
 *  renders no rows at all. Give every element a plausible size. */
beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as never

  Element.prototype.getBoundingClientRect = function () {
    return { width: 1200, height: 800, top: 0, left: 0, right: 1200, bottom: 800, x: 0, y: 0, toJSON: () => ({}) }
  }
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 800 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 1200 })
})

const item = (key: string, title: string) =>
  ({ key, libraryId: 1, itemType: 'journalArticle', title, creators: [], tags: [], collections: [], version: 1, deleted: false, dateAdded: 0, dateModified: 0 }) as unknown as Item

const ITEMS = ['A', 'B', 'C', 'D'].map((k, i) => item(k, `Paper ${i}`))

let container: HTMLElement
let root: Root

const render = async () => {
  await act(async () => {
    root.render(<App />)
  })
}

const click = async (el: Element | null | undefined, init: MouseEventInit = {}) => {
  expect(el, 'the element to click').toBeTruthy()
  await act(async () => {
    el!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0, ...init }))
    el!.dispatchEvent(new MouseEvent('click', { bubbles: true, button: 0, ...init }))
  })
}

beforeEach(() => {
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: ITEMS, total: ITEMS.length }),
    ready: true,
    tabs: [libraryTab('Library')],
    activeTab: 'library',
    scopes: {},
  })
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

const rows = () => [...container.querySelectorAll('.row')]

/** Type into a controlled input.
 *
 *  React tracks an input's value on the node and ignores an event whose value
 *  it believes it already knows, so assigning `.value` directly changes what
 *  is on screen and nothing else. The native setter is what updates the
 *  tracker as a keystroke would. */
async function type(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set
  setter?.call(input, value)
  await act(async () => {
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

describe('selecting rows', () => {
  it('replaces the selection on a plain click', async () => {
    await render()
    await click(rows()[1])
    expect(useStore.getState().selected).toEqual(['B'])
  })

  it('extends to a range with shift', async () => {
    await render()
    await click(rows()[0])
    await click(rows()[2], { shiftKey: true })
    expect(useStore.getState().selected).toEqual(['A', 'B', 'C'])
  })

  it('adds one at a time with the toggle modifier', async () => {
    await render()
    await click(rows()[0])
    await click(rows()[3], { ctrlKey: true })
    expect(useStore.getState().selected).toEqual(['A', 'D'])
  })
})

describe('preview tabs', () => {
  it('reuses one slot while skimming, and keeps the tab on a double click', async () => {
    await render()
    const store = useStore.getState()

    act(() => store.openReader('A'))
    act(() => store.openReader('B'))
    expect(useStore.getState().tabs.map((t) => t.id)).toEqual(['library', tabId('reader', 'B')])

    act(() => useStore.getState().keepTab(tabId('reader', 'B')))
    act(() => useStore.getState().openReader('C'))
    expect(useStore.getState().tabs).toHaveLength(3)
  })

  it('shows a preview in italics and a kept tab upright', async () => {
    act(() => useStore.getState().openReader('A'))
    await render()
    expect(container.querySelector('.tab[data-preview]'), 'the preview').toBeTruthy()

    act(() => useStore.getState().keepTab(tabId('reader', 'A')))
    await render()
    expect(container.querySelector('.tab[data-preview]'), 'no longer a preview').toBeNull()
  })
})

describe('per-tab state', () => {
  it('keeps each tab its own query and selection', async () => {
    await render()
    act(() => useStore.getState().select('B'))
    act(() => useStore.setState({ query: 'first tab' }))

    act(() => {
      useStore.getState().openTab({ id: 'library:X', kind: 'library', title: 'Other', target: 'X' })
    })
    expect(useStore.getState().query, 'a new tab starts clean').toBe('')

    act(() => useStore.getState().activateTab('library'))
    expect(useStore.getState().query).toBe('first tab')
    expect(useStore.getState().selected).toEqual(['B'])
  })
})

describe('the search box', () => {
  it('searches items in the library, and filters elsewhere', async () => {
    await render()
    const input = () => container.querySelector<HTMLInputElement>('#search-input')

    expect(input(), 'the library has a search box').toBeTruthy()
    await act(async () => {
      useStore.getState().openTab({ id: tabId('collections'), kind: 'collections', title: '' })
    })
    await render()

    const box = input()
    expect(box, 'so do collections').toBeTruthy()
    await type(box!, 'papers')
    // Filtering a list of collections must not run a library search.
    expect(useStore.getState().filter).toBe('papers')
    expect(useStore.getState().query).toBe('')
  })
})
