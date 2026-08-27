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
