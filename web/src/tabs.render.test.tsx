/**
 * Tab navigation, driven through the workbench.
 *
 * The rules these assert are written down in docs/16-workspace-rules.md. They
 * are asserted here rather than in `lib/tabs.ts` alone because the confusing
 * part was never the pure function — it was what the sidebar, the list and the
 * chat actually *ask* for, and whether the scope each tab owns survives the
 * trip.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Conversation, Item } from './api/types'
import { libraryTab } from './lib/tabs'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: () => {
        if (path === 'api.conversations.messages') return Promise.resolve([])
        if (path === 'api.conversations.list') return Promise.resolve([])
        if (path === 'api.conversations.append') return Promise.resolve({})
        if (path === 'api.conversations.ask') return Promise.resolve({})
        if (path === 'api.conversations.rename') return Promise.resolve({})
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

const conversation = (key: string, title: string) =>
  ({ key, libraryId: 1, title, messageCount: 2 }) as Conversation

let container: HTMLElement
let root: Root

beforeEach(() => {
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [item('A'), item('B')], total: 2 }),
    ready: true,
    tabs: [libraryTab('Library')],
    activeTab: 'library',
    scopes: {},
    collections: [],
    smartCollections: [],
    tags: [],
    badgeDefs: [],
    conversations: [conversation('C1', 'First thread'), conversation('C2', 'Second thread')],
    conversation: null,
    messages: [],
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

const store = () => useStore.getState()
const ids = () => store().tabs.map((t) => t.id)
const previews = () => store().tabs.filter((t) => t.preview).map((t) => t.id)

describe('the tab model', () => {
  it('starts with one library tab that cannot be closed', async () => {
    await render()

    expect(ids()).toEqual(['library'])
    await act(async () => store().closeTab('library'))
    expect(ids()).toEqual(['library'])
  })

  it('gives a glance one slot, whatever kind it is', async () => {
    await render()

    await act(async () => store().openReader('A'))
    await act(async () => store().openGraph('B'))

    // Clicking through a list must not leave a tab behind for every glance,
    // and the slot is shared across kinds — otherwise each kind leaks one.
    expect(previews()).toHaveLength(1)
    expect(ids()).toEqual(['library', 'graph:B'])
  })

  it('keeps a tab the reader asked to keep, and stops reusing its slot', async () => {
    await render()
    await act(async () => store().openReader('A'))
    await act(async () => store().keepTab('reader:A'))
    await act(async () => store().openGraph('B'))

    expect(ids()).toEqual(['library', 'reader:A', 'graph:B'])
    expect(previews()).toEqual(['graph:B'])
  })

  it('focuses an open tab rather than opening a second one', async () => {
    await render()
    await act(async () => store().openReader('A'))
    await act(async () => store().keepTab('reader:A'))
    await act(async () => store().activateTab('library'))
    await act(async () => store().openReader('A'))

    expect(ids()).toEqual(['library', 'reader:A'])
    expect(store().activeTab).toBe('reader:A')
  })

  it('never demotes a kept tab back to a glance', async () => {
    await render()
    await act(async () => store().openReader('A'))
    await act(async () => store().keepTab('reader:A'))
    await act(async () => store().openReader('A'))

    expect(previews()).toEqual([])
  })

  it('opens a conversation as a glance and reuses the slot for the next', async () => {
    await render()

    await act(async () => store().openConversation('C1'))
    await act(async () => store().openConversation('C2'))

    // Reading through a list of past threads is browsing, not opening: the
    // same gesture as clicking through papers, so the same rule.
    expect(ids()).toEqual(['library', 'chat:C2'])
    expect(store().conversation).toBe('C2')
  })

  it('keeps a conversation the moment it is written in', async () => {
    await render()
    await act(async () => store().openConversation('C1'))
    expect(previews()).toEqual(['chat:C1'])

    await act(async () => store().sendMessage('does this stay?'))

    // Typing into a surface is the clearest possible statement that it is not
    // a glance.
    expect(previews()).toEqual([])
    expect(ids()).toContain('chat:C1')
  })

  it('leaves the chat tab alone when the library is brought forward', async () => {
    await render()
    await act(async () => store().openConversation('C1'))
    await act(async () => store().activateTab('library'))

    expect(store().activeTab).toBe('library')
    expect(ids()).toContain('chat:C1')
    // Switching away must not quietly promote or discard it.
    expect(previews()).toEqual(['chat:C1'])
  })

  it('brings the library back with what it was showing', async () => {
    await render()
    await act(async () => store().select('B'))
    const before = store().selected

    await act(async () => store().openConversation('C1'))
    await act(async () => store().activateTab('library'))

    expect(store().selected).toEqual(before)
    expect(store().items).toHaveLength(2)
  })

  it('opens each singleton surface once, and pinned', async () => {
    await render()

    await act(async () => store().openSettings())
    await act(async () => store().openSettings())

    expect(ids()).toEqual(['library', 'settings'])
    // A destination is not a glance: it does not get replaced by the next one.
    expect(previews()).toEqual([])
  })

  it('shows the tab after the closed one, then the one before', async () => {
    await render()
    await act(async () => store().openSettings())
    await act(async () => store().openReader('A'))
    await act(async () => store().keepTab('reader:A'))
    await act(async () => store().activateTab('settings'))

    await act(async () => store().closeTab('settings'))

    expect(store().activeTab).toBe('reader:A')
  })

  it('falls back to the library when the last tab closes', async () => {
    await render()
    await act(async () => store().openSettings())
    await act(async () => store().closeTab('settings'))

    expect(store().activeTab).toBe('library')
  })

  it('forgets a closed tab rather than leaving its list in memory', async () => {
    await render()
    await act(async () => store().openReader('A'))
    await act(async () => store().keepTab('reader:A'))
    await act(async () => store().activateTab('library'))
    await act(async () => store().closeTab('reader:A'))

    expect(Object.keys(store().scopes)).not.toContain('reader:A')
  })
})
