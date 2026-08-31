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
let children: Record<string, unknown>[] = []
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
        if (path === 'api.items.children') return Promise.resolve(children)
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
  children = [
    {
      key: 'NOTE0001',
      itemType: 'note',
      note: '<p>The dataset is the real contribution.</p>',
      tags: [{ tag: 'summary', type: 1 }],
    },
    {
      key: 'NOTE0002',
      itemType: 'note',
      note: '<p>My own reading note.</p>',
      tags: [],
    },
    { key: 'FILE0001', itemType: 'attachment', tags: [] },
  ]
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

/** Open one of the detail pane's tabs.
 *
 *  The pane is a column and five sections stacked in it meant scrolling past
 *  the whole record to reach the references, so each now answers its own
 *  question behind a tab. These tests name the tab they are about. */
async function openTab(label: RegExp) {
  const tab = [...container.querySelectorAll('.detail-tabs .rail-tab')].find((b) =>
    label.test(b.textContent ?? ''),
  )
  if (!tab) throw new Error(`no detail tab matching ${label}`)
  await act(async () => {
    tab.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  })
}

describe('the detail pane\'s tabs', () => {
  it('scrolls the content, not the title and tabs', async () => {
    await render()

    // The pane used to scroll as a whole, so a long abstract carried the tab
    // strip off the top and you could not tell which tab you were in, let
    // alone reach another.
    const body = container.querySelector('.detail-body')
    const tabs = container.querySelector('.detail-tabs')
    expect(body, 'no scrolling body').toBeTruthy()
    expect(tabs, 'no tab strip').toBeTruthy()
    expect(body?.contains(tabs ?? null), 'the tabs scroll with the content').toBe(false)
  })

  it('does not label a section with the name of its own tab', async () => {
    await render()

    // "Notes" printed beside the Notes tab is the same word twice, taking a
    // quarter of a narrow column to say nothing.
    await openTab(/Notes|笔记/)
    const section = container.querySelector('.detail-section[data-section="notes"]')
    expect(section, 'the notes section still uses the label/value frame').toBeTruthy()
    expect(section?.closest('.field-grid'), 'still inside the two-column grid').toBeNull()
  })

  it('shows one section at a time', async () => {
    await render()

    // The reason the tabs exist: an abstract can fill the pane on its own, so
    // the references used to be a scroll away rather than a click.
    await openTab(/References|参考/)
    expect(container.querySelector('.reference-list, .chip-row')).toBeTruthy()
    expect(
      container.textContent,
      'the threads section is showing while References is chosen',
    ).not.toContain('Why does this work?')

    await openTab(/Threads|对话/)
    expect(container.textContent).toContain('Why does this work?')
  })
})

describe("a paper's conversations", () => {
  it('asks about the selected paper', async () => {
    await render()
    await openTab(/Threads|对话/)
    expect(asked).toContain('A')
  })

  it('lists the threads that named it', async () => {
    await render()
    await openTab(/Threads|对话/)
    expect(container.textContent).toContain('Why does this work?')
  })

  it('says so plainly when nothing has been asked', async () => {
    about = []
    await render()
    await openTab(/Threads|对话/)
    // An empty section that renders nothing is indistinguishable from one
    // that failed to load.
    expect(container.textContent).toContain('Nothing asked about this yet')
  })

  it('offers a way to start one', async () => {
    await render()
    await openTab(/Threads|对话/)
    const buttons = [...container.querySelectorAll('.detail button')].map((b) => b.textContent)
    expect(buttons.some((label) => label?.includes('Ask about this'))).toBe(true)
  })
})

describe("a paper's references", () => {
  it('lists what the paper stands on', async () => {
    await render()
    await openTab(/References|参考/)
    // Stored since citations arrived and used by the graph, with nowhere to
    // read it plainly until now.
    expect(container.textContent).toContain('A work it cites')
    expect(container.textContent).toContain('Something we do not have')
  })

  it('says how many of them the library holds', async () => {
    await render()
    await openTab(/References|参考/)
    expect(container.textContent).toContain('1 of 2 in your library')
  })

  it('links only the ones that go somewhere', async () => {
    await render()
    await openTab(/References|参考/)
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
    await openTab(/References|参考/)
    const buttons = [...container.querySelectorAll('.detail button')].map((b) => b.textContent)
    expect(buttons.some((label) => label?.includes('Fetch'))).toBe(true)
  })
})

describe("a paper's notes", () => {
  it('shows what has been written under the paper', async () => {
    await render()
    await openTab(/Notes|笔记/)
    // Summarising has landed a note under the item since it was built, and
    // nothing showed it: you had to know to look at the item's children.
    expect(container.textContent).toContain('The dataset is the real contribution')
    expect(container.textContent).toContain('My own reading note')
  })

  it('strips the markup for the preview', async () => {
    await render()
    await openTab(/Notes|笔记/)
    const row = container.querySelector('.note-text')
    expect(row?.textContent).not.toContain('<p>')
  })

  it('marks the ones the model wrote', async () => {
    await render()
    await openTab(/Notes|笔记/)
    // A summary the model wrote and a note the user wrote are different
    // things to trust.
    const rows = [...container.querySelectorAll('.note-row')]
    expect(rows[0]?.querySelector('.note-badge')).not.toBeNull()
    expect(rows[1]?.querySelector('.note-badge')).toBeNull()
  })

  /// The section used to disappear when a paper had no notes, so the one
  /// place you would go to write your first note was the one place that was
  /// not there until you already had one.
  it('still offers a way to write the first one', async () => {
    children = []
    await render()
    await openTab(/Notes|笔记/)
    const add = container.querySelector('.note-row.add')
    expect(add, 'a paper with no notes must still offer to take one').not.toBeNull()
    expect(container.querySelectorAll('.note-row')).toHaveLength(1)
  })
})
