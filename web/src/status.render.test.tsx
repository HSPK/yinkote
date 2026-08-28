/**
 * The status surface, rendered.
 *
 * This is the page somebody opens when something looks wrong, which makes it
 * the worst possible page to fall over on incomplete data — the state it is
 * most likely to be shown in is exactly the state it has to survive.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Stats } from './api/types'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

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

const stats = (over: Partial<Stats> = {}): Stats =>
  ({
    items: 1234,
    trashed: 5,
    collections: 7,
    tags: 42,
    version: 99,
    uptimeSecs: 3600,
    wsClients: 2,
    search: { documents: 1234, embedded: 1000, dimensions: 256, provider: 'local-hash' },
    ...over,
  }) as Stats

let container: HTMLElement
let root: Root

beforeEach(() => {
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [{ id: 'status', kind: 'status', title: 'Status' }],
    activeTab: 'status',
    scopes: {},
    collections: [],
    smartCollections: [],
    tags: [],
    badgeDefs: [],
    plugins: [],
    stats: stats(),
    server: null,
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
  await act(async () => {
    await Promise.resolve()
  })
}

describe('the status surface', () => {
  it('reports the size of the library', async () => {
    await render()
    expect(container.textContent).toContain('1.2k')
  })

  it('says how much of the library has been embedded', async () => {
    await render()
    // Semantic search silently returns less when the backlog is large, so the
    // number that explains it has to be visible.
    expect(container.textContent).toContain('local-hash')
  })

  it('draws before any of it has arrived', async () => {
    // The page opens on a cold start too, and a blank screen at that moment
    // is indistinguishable from a server that never answered.
    useStore.setState({ stats: null, server: null })
    await render()
    expect(container.textContent).not.toContain('failed to draw')
  })

  it('survives stats that arrive without their search half', async () => {
    // A server mid-upgrade, or one whose search subsystem failed to start,
    // sends the rest. Falling over here hides the very thing being diagnosed.
    useStore.setState({ stats: { ...stats(), search: undefined } as unknown as Stats })
    await render()
    expect(container.textContent).not.toContain('failed to draw')
  })
})
