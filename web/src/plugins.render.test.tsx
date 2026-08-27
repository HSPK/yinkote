/**
 * The plugin surface, rendered.
 *
 * A plugin system is only complete if the things it promises are reachable:
 * seeing what is installed, seeing what each one contributes, turning one off,
 * and calling into it when it misbehaves. Each of those is a wire that can be
 * missing without anything looking wrong.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { PluginStatus } from './api/types'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

const toggled: { id: string; enabled: boolean }[] = []
const called: { id: string; method: string }[] = []
let rescans = 0
let installed: PluginStatus[] = []

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: (_t, _this, args: unknown[]) => {
        if (path === 'api.plugins.setEnabled') {
          toggled.push({ id: String(args[0]), enabled: Boolean(args[1]) })
          return Promise.resolve({ ok: true })
        }
        if (path === 'api.plugins.call') {
          called.push({ id: String(args[0]), method: String(args[1]) })
          return Promise.resolve({ pong: true })
        }
        if (path === 'api.plugins.reload') {
          rescans += 1
          return Promise.resolve(installed)
        }
        // Answering with the same list rather than an empty one: toggling a
        // plugin reloads, and a mock that forgets everything would make the
        // page look broken when it is the fixture that vanished.
        if (path === 'api.plugins.list') return Promise.resolve(installed)
        return new Promise(() => {})
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
  Element.prototype.scrollIntoView = () => {}
  Element.prototype.getBoundingClientRect = () =>
    ({ width: 1200, height: 800, top: 0, left: 0, right: 1200, bottom: 800, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 800 })
})

/** Shaped from what the server actually sends, not from the client's type.
 *
 *  The first version of this fixture left out `avgLatencyMs`, and the panel
 *  did not render a dash — it crashed outright, taking every plugin with it.
 *  Both ends were wrong, and only rendering showed either. */
const plugin = (over: Partial<PluginStatus>): PluginStatus => ({
  id: 'crossref',
  name: 'Crossref',
  version: '1.0.0',
  state: 'ready',
  description: 'Fills in metadata from a DOI.',
  capabilities: ['metadata_source'],
  permissions: ['network'],
  hooks: [],
  contributions: {
    metadataSources: [{ id: 'crossref', label: 'Crossref', pluginId: 'crossref' }],
    importers: [],
    exporters: [],
    itemActions: [],
    badges: [],
  },
  calls: 1,
  failures: 0,
  avgLatencyMs: 31,
  source: 'plugins/crossref',
  ...over,
})

let container: HTMLElement
let root: Root

beforeEach(() => {
  toggled.length = 0
  called.length = 0
  rescans = 0
  // Distinct in every visible field. Sharing a contribution label made the
  // first version of these tests act on the wrong card every time, because
  // "find the card mentioning Crossref" matched a chip on somebody else's.
  installed = [
    plugin({ id: 'crossref', name: 'Crossref' }),
    plugin({
      id: 'auto-tag',
      name: 'Auto tag',
      state: 'disabled',
      description: 'Suggests tags.',
      contributions: {
        metadataSources: [],
        importers: [],
        exporters: [],
        itemActions: [{ id: 'retag', label: 'Retag', pluginId: 'auto-tag' }],
        badges: [],
      },
    }),
    plugin({
      id: 'broken',
      name: 'Broken one',
      state: 'failed',
      error: 'could not start',
      description: 'Never got going.',
      contributions: {
        metadataSources: [],
        importers: [],
        exporters: [],
        itemActions: [],
        badges: [],
      },
    }),
  ]
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [{ id: 'plugins', kind: 'plugins', title: 'Plugins' }],
    activeTab: 'plugins',
    scopes: {},
    collections: [],
    smartCollections: [],
    tags: [],
    badgeDefs: [],
    plugins: installed,
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

const cards = () => [...container.querySelectorAll('article')]
const buttonIn = (card: Element, label: string) =>
  [...card.querySelectorAll('button')].find((b) => b.textContent?.includes(label)) as HTMLButtonElement

describe('the plugin surface', () => {
  it('lists what is installed', async () => {
    await render()
    expect(cards()).toHaveLength(3)
  })

  it('puts what is broken at the top', async () => {
    await render()
    // A plugin that failed to start is the only one anybody needs to act on,
    // and it is the one that sorts last alphabetically.
    expect(cards()[0]?.textContent).toContain('Broken one')
  })

  it('says why a plugin failed rather than only that it did', async () => {
    await render()
    expect(cards()[0]?.textContent).toContain('could not start')
  })

  it('turns one off, and offers to turn a disabled one back on', async () => {
    await render()

    const disabled = cards().find((c) => c.textContent?.includes('Auto tag'))!
    expect(buttonIn(disabled, 'Enable')).toBeTruthy()
    await act(async () => buttonIn(disabled, 'Enable').click())
    expect(toggled).toEqual([{ id: 'auto-tag', enabled: true }])

    const ready = cards().find((c) => c.textContent?.includes('Crossref'))!
    await act(async () => buttonIn(ready, 'Disable').click())
    expect(toggled[1]).toEqual({ id: 'crossref', enabled: false })
  })

  it('rescans on request', async () => {
    await render()
    const rescan = [...container.querySelectorAll('button')].find((b) =>
      b.textContent?.includes('Rescan'),
    ) as HTMLButtonElement
    await act(async () => rescan.click())
    expect(rescans).toBe(1)
  })

  it('aims the console at the plugin whose button was pressed', async () => {
    await render()

    const ready = cards().find((c) => c.textContent?.includes('Crossref'))!
    await act(async () => buttonIn(ready, 'Call').click())

    // Otherwise "Call" is a button that scrolls and does nothing else.
    const select = container.querySelector('#plugin-console select') as HTMLSelectElement
    expect(select?.value).toBe('crossref')
  })

  it('survives a plugin whose numbers are missing', async () => {
    // Writing this fixture without `avgLatencyMs` is how the panel was found
    // to crash outright rather than show a dash — an older server, or a host
    // that does not time its calls, would have taken every plugin off screen.
    installed = [{ ...plugin({}), avgLatencyMs: undefined as unknown as number }]
    useStore.setState({ plugins: installed })
    await render()

    expect(container.textContent).not.toContain('failed to draw')
    expect(cards()).toHaveLength(1)
    expect(cards()[0]?.textContent).toContain('—')
  })

  it('calls the plugin with what was typed', async () => {
    await render()

    const ready = cards().find((c) => c.textContent?.includes('Crossref'))!
    await act(async () => buttonIn(ready, 'Call').click())

    const send = [...container.querySelectorAll('#plugin-console button')].find((b) =>
      b.textContent?.includes('Send'),
    ) as HTMLButtonElement
    await act(async () => send.click())

    expect(called[0]).toEqual({ id: 'crossref', method: 'initialize' })
    expect(container.querySelector('#plugin-console')?.textContent).toContain('pong')
  })
})
