/**
 * The table header must not claim a sort the server did not apply.
 *
 * A ranked search scores a bounded pool of candidates and returns it
 * best-first; the requested column sort is ignored. For a long time it was
 * ignored *silently*: the header still drew its arrow, still accepted clicks,
 * and the rows never moved. A user could reasonably conclude the list was in
 * title order and read the first row as the first title in their library.
 *
 * Sorting the retrieved pool instead would be worse than doing nothing — it
 * would answer "the first title among the best three hundred hits" while
 * looking exactly like "the first title in the library". So the honest
 * behaviour is to stop claiming, and to say what the order actually is.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Item } from './api/types'
import { enUS } from './i18n/en-US'
import { libraryTab } from './lib/tabs'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

const item = (title: string): Item =>
  ({
    key: title.replace(/\W/g, ''),
    libraryId: 1,
    itemType: 'journalArticle',
    title,
    tags: [],
    collections: [],
    creators: [],
    dateAdded: 0,
    dateModified: 0,
    version: 1,
  }) as unknown as Item

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
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
    ({ width: 1200, height: 800, top: 0, left: 0, bottom: 800, right: 1200 }) as DOMRect
})

let root: Root
let host: HTMLElement

function show(ranked: boolean) {
  useStore.setState({
    ...emptyScope({ items: [item('Alpha'), item('Beta')], total: 2 }),
    ready: true,
    tabs: [libraryTab('Library')],
    activeTab: 'library',
    scopes: {},
    collections: [],
    smartCollections: [],
    tags: [],
    badgeDefs: [],
    sort: 'title',
    direction: 'asc',
    ranked,
    query: ranked ? 'transformer' : '',
  })
  act(() => {
    root = createRoot(host)
    root.render(<App />)
  })
}

/** The header button for a column, by its visible label. */
const head = (label: string) =>
  [...host.querySelectorAll<HTMLButtonElement>('.table-head button')].find(
    (b) => (b.textContent ?? '').includes(label),
  )

beforeEach(() => {
  host = document.createElement('div')
  document.body.append(host)
})

afterEach(() => {
  act(() => root.unmount())
  host.remove()
})

describe('sorting during a ranked search', () => {
  it('draws the arrow and takes clicks when the sort is real', () => {
    show(false)
    const title = head(enUS['table.title'])
    expect(title, 'no title column on the page').toBeTruthy()
    expect(title!.disabled).toBe(false)
    expect(title!.textContent).toContain('↑')
  })

  it('draws no arrow while results are ranked', () => {
    show(true)
    // The arrow is the claim. Nothing on the header row may assert an order
    // the rows are not in.
    expect(host.querySelector('.table-head .sort-arrow')).toBeNull()
  })

  it('refuses the click rather than pretending to act on it', () => {
    show(true)
    expect(head(enUS['table.title'])!.disabled).toBe(true)
    expect(head(enUS['table.title'])!.title).toBe(enUS['table.rankedHint'])
  })

  it('says what the order actually is', () => {
    show(true)
    // Removing the arrow leaves a question the user did not have before, so
    // the answer has to appear somewhere.
    expect(host.textContent ?? '').toContain(enUS['table.ranked'])
  })

  it('says nothing about relevance when simply browsing', () => {
    show(false)
    expect(host.textContent ?? '').not.toContain(enUS['table.ranked'])
  })
})
