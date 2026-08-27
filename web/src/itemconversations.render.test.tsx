/**
 * A paper's own history, rendered.
 *
 * The detail panel answers "what did I already work out about this?" without
 * making the reader remember which thread it was in. Only rendering shows
 * whether the panel actually asks — a section wired to nothing looks exactly
 * like one wired to an empty answer.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Conversation, Item } from './api/types'
import { libraryTab } from './lib/tabs'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

let about: Conversation[] = []
let references: { position: number; key: string | null; label: string; year: number | null; fingerprint: string }[] = []
const asked: string[] = []

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: (_t, _this, args: unknown[]) => {
        if (path === 'api.conversations.aboutItem') {
          asked.push(String(args[1] ?? ''))
          return Promise.resolve({ conversations: about })
        }
        if (path === 'api.references.list') {
          return Promise.resolve({ cites: references, citedBy: [], resolved: 1 })
        }
        if (path === 'api.schema') return new Promise(() => {})
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
  Element.prototype.getBoundingClientRect = () =>
    ({ width: 1200, height: 800, top: 0, left: 0, right: 1200, bottom: 800, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 800 })
})

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
  asked.length = 0
  references = [
    { position: 0, key: 'HELD0001', label: 'A work it cites', year: 2019, fingerprint: 'doi:1' },
    { position: 1, key: null, label: 'Something we do not have', year: 2020, fingerprint: 'doi:2' },
  ]
  about = [
    { key: 'CONV0001', libraryId: 1, title: 'Why does this work?' } as Conversation,
  ]
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [item('A')], total: 1 }),
    ready: true,
    tabs: [libraryTab('Library')],
    activeTab: 'library',
    scopes: {},
    collections: [],
    smartCollections: [],
    tags: [],
    badgeDefs: [],
    selected: ['A'],
    panel: 'detail',
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
  await act(async () => {
    await Promise.resolve()
  })
}

describe("a paper's conversations", () => {
  it('asks about the selected paper', async () => {
    await render()
    expect(asked).toContain('A')
  })

  it('lists the threads that named it', async () => {
    await render()
    expect(container.textContent).toContain('Why does this work?')
  })

  it('says so plainly when nothing has been asked', async () => {
    about = []
    await render()
    // An empty section that renders nothing is indistinguishable from one
    // that failed to load.
    expect(container.textContent).toContain('Nothing asked about this yet')
  })

  it('offers a way to start one', async () => {
    await render()
    const buttons = [...container.querySelectorAll('.detail button')].map((b) => b.textContent)
    expect(buttons.some((label) => label?.includes('Ask about this'))).toBe(true)
  })
})

describe("a paper's references", () => {
  it('lists what the paper stands on', async () => {
    await render()
    // Stored since citations arrived and used by the graph, with nowhere to
    // read it plainly until now.
    expect(container.textContent).toContain('A work it cites')
    expect(container.textContent).toContain('Something we do not have')
  })

  it('says how many of them the library holds', async () => {
    await render()
    expect(container.textContent).toContain('1 of 2 in your library')
  })

  it('links only the ones that go somewhere', async () => {
    await render()
    // Clicking a work the library does not hold does nothing, so it must not
    // look clickable.
    const rows = [...container.querySelectorAll('.reference-row')]
    expect(rows[0]?.querySelector('button')).not.toBeNull()
    expect(rows[1]?.querySelector('button')).toBeNull()
    expect(rows[1]?.querySelector('.reference-absent')).not.toBeNull()
  })

  it('offers to fetch when there are none', async () => {
    references = []
    await render()
    const buttons = [...container.querySelectorAll('.detail button')].map((b) => b.textContent)
    expect(buttons.some((label) => label?.includes('Fetch'))).toBe(true)
  })
})
