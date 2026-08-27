/**
 * Duplicates, rendered.
 *
 * Merging is the one gesture in the workbench a user cannot undo by hand, so
 * what is tested here is the part that protects them: which record survives is
 * the one they pointed at, the others are named in the request rather than
 * guessed at, and the group leaves the screen once it is no longer a group.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Item } from './api/types'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

const merges: { master: string; others: string[] }[] = []
let groups: Item[][] = []

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: (_t, _this, args: unknown[]) => {
        if (path === 'api.duplicates.groups') {
          return Promise.resolve({ groups, total: groups.length })
        }
        if (path === 'api.duplicates.merge') {
          merges.push({ master: args[1] as string, others: args[2] as string[] })
          return Promise.resolve({ item: groups[0]?.[0], merged: (args[2] as string[]).length })
        }
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

let container: HTMLElement
let root: Root

const item = (key: string, extra: Partial<Item> = {}): Item => ({
  key,
  libraryId: 1,
  itemType: 'journalArticle',
  creators: [{ creatorType: 'author', lastName: 'Zhang', firstName: 'A' }],
  tags: [],
  collections: [],
  version: 1,
  deleted: false,
  dateAdded: 1_700_000_000_000,
  dateModified: 1_700_000_000_000,
  title: 'Attention Is All You Need',
  date: '2017',
  ...extra,
})

beforeEach(() => {
  merges.length = 0
  groups = [
    [
      item('THIN0001'),
      // The fuller copy: a PDF, a tag, a collection. Distinct from the other so
      // a click cannot land on the wrong one and still pass.
      item('FULL0001', {
        attachments: ['pdf'],
        tags: [{ tag: 'transformers', type: 0 }],
        collections: ['COLL0001'],
        publicationTitle: 'NeurIPS',
      }),
    ],
  ]

  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [{ id: 'duplicates', kind: 'duplicates', title: 'Duplicates' }],
    activeTab: 'duplicates',
    scopes: {},
    collections: [],
    smartCollections: [],
    tags: [],
    badgeDefs: [],
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

const rows = () => [...container.querySelectorAll('.dup-row')]

describe('duplicates', () => {
  it('shows each copy of a group with what is only on it', async () => {
    await render()

    expect(container.querySelectorAll('.dup-group')).toHaveLength(1)
    expect(rows()).toHaveLength(2)
    // The fuller copy advertises its attachment, tag and collection, because
    // that is the whole basis for choosing between two otherwise equal records.
    expect(rows()[1]?.querySelector('.attach-mark')).not.toBeNull()
    expect(rows()[1]?.textContent).toContain('1 tags')
    expect(rows()[1]?.textContent).toContain('1 collections')
    expect(rows()[0]?.querySelector('.attach-mark')).toBeNull()
  })

  it('keeps the record that was pointed at and names the rest', async () => {
    await render()

    const keep = rows()[1]?.querySelector('button') as HTMLButtonElement
    await act(async () => keep.click())

    expect(merges).toEqual([{ master: 'FULL0001', others: ['THIN0001'] }])
  })

  it('takes the group off the screen once it is one record', async () => {
    await render()
    const keep = rows()[0]?.querySelector('button') as HTMLButtonElement
    await act(async () => keep.click())

    // Not a reload: the rest of the list is still true, and re-fetching would
    // throw away the reader's place in a screen they are working down.
    expect(container.querySelectorAll('.dup-group')).toHaveLength(0)
  })
})
