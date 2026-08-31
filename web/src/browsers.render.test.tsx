/**
 * The two browsers that sit behind a "browse all" link.
 *
 * Both exist because a sidebar is a shortcut list: when it stops fitting, the
 * rest has to live somewhere that can be sorted and searched. So these tests
 * are about the two things such a surface owes — that everything is listed, and
 * that the ordering the header offers is the ordering you get.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Collection, Conversation } from './api/types'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'
import { CHAT_DEFAULT_VISIBLE, COLLECTION_DEFAULT_VISIBLE } from './lib/columns'

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
})

const DAY = 86_400_000

const collection = (name: string, itemCount: number, dateAdded: number): Collection => ({
  key: name.toUpperCase().padEnd(8, 'X').slice(0, 8),
  libraryId: 1,
  name,
  sortIndex: 0,
  version: 1,
  dateAdded,
  dateModified: dateAdded,
  itemCount,
})

const conversation = (title: string, messageCount: number, updatedAt: number): Conversation => ({
  key: title.toUpperCase().padEnd(8, 'X').slice(0, 8),
  libraryId: 1,
  title,
  messageCount,
  createdAt: updatedAt - DAY,
  updatedAt,
})

let container: HTMLElement
let root: Root

beforeEach(() => {
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

async function show(kind: 'collections' | 'chats', state: Record<string, unknown>) {
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [{ id: kind, kind, title: '' }],
    activeTab: kind,
    scopes: {},
    collections: [],
    smartCollections: [],
    conversations: [],
    tags: [],
    badgeDefs: [],
    columnOrders: { items: [], collections: COLLECTION_DEFAULT_VISIBLE, chats: CHAT_DEFAULT_VISIBLE },
    ...state,
  })
  await act(async () => {
    root.render(<App />)
  })
  await act(async () => {
    await Promise.resolve()
  })
}

const cells = (selector: string) =>
  [...container.querySelectorAll(selector)].map((r) => r.textContent ?? '')

describe('the collection browser', () => {
  const shelves = [
    collection('Older', 3, 1_700_000_000_000),
    collection('Newer', 1, 1_800_000_000_000),
  ]

  it('shows when each collection was made', async () => {
    await show('collections', { collections: shelves })

    const rows = cells('.browser-grid.row')
    expect(rows).toHaveLength(2)
    // A date, not a blank: the column is the whole point of the migration.
    expect(rows.some((r) => /\d{4}-\d{2}-\d{2}|\d{2}-\d{2} \d{2}:\d{2}/.test(r))).toBe(true)
  })

  it('says so rather than inventing a date it does not have', async () => {
    await show('collections', { collections: [collection('Ancient', 2, 0)] })

    // Rows written before the library recorded dates have none. Showing today
    // would make every old collection look new.
    expect(cells('.browser-grid.row')[0]).toContain('—')
  })

  it('sorts by creation date when that column is chosen', async () => {
    await show('collections', { collections: shelves })

    const created = [...container.querySelectorAll('.table-head button')].find((b) =>
      /Added|添加/.test(b.textContent ?? ''),
    )
    expect(created, 'no sortable created column in the header').toBeTruthy()

    await act(async () => {
      created!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    expect(cells('.browser-grid.row')[0]).toContain('Older')
  })

  it('lines the rows up with the headings', async () => {
    await show('collections', { collections: shelves })

    // The header carried the chosen columns and the rows fell back to the
    // stylesheet's fixed five, so every cell sat under the wrong heading. It
    // showed up when the detail panel opened, because that is when the pane
    // narrows enough for the two track lists to disagree visibly.
    const head = container.querySelector('.table-head.browser-grid') as HTMLElement
    const rows = [...container.querySelectorAll('.browser-grid.row')] as HTMLElement[]

    expect(head, 'no header rendered').toBeTruthy()
    expect(rows.length).toBeGreaterThan(0)
    for (const row of rows) {
      expect(row.style.gridTemplateColumns, 'a row does not use the header tracks').toBe(
        head.style.gridTemplateColumns,
      )
    }
    expect(head.style.gridTemplateColumns).not.toBe('')
  })

  it('drops a column the user has turned off', async () => {
    await show('collections', {
      collections: shelves,
      columnOrders: { items: [], collections: ['name', 'items'], chats: CHAT_DEFAULT_VISIBLE },
    })

    // The rule column is not among the chosen ones, so the header must not
    // offer it — a table that ignores the picker is a picker that lies.
    const headers = cells('.table-head button')
    expect(headers.some((h) => /Rule|规则/.test(h))).toBe(false)
    expect(headers.some((h) => /Name|名称/.test(h))).toBe(true)
  })
})

describe('the conversation browser', () => {
  const threads = [
    conversation('Screen time', 2, 1_800_000_000_000),
    conversation('Wastewater', 9, 1_700_000_000_000),
  ]

  it('lists every conversation with its length', async () => {
    await show('chats', { conversations: threads })

    const rows = cells('.chats-grid.row')
    expect(rows).toHaveLength(2)
    expect(rows.join(' ')).toContain('Wastewater')
    expect(rows.join(' ')).toContain('9')
  })

  it('describes the clicked conversation without opening it', async () => {
    await show('chats', { conversations: threads })

    const row = [...container.querySelectorAll('.chats-grid.row')].find((r) =>
      (r.textContent ?? '').includes('Wastewater'),
    )
    await act(async () => {
      row?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })

    // Inspected, not opened: the conversation a chat tab is showing must not
    // change because somebody clicked a row in a list.
    expect(useStore.getState().inspectedChat).toBe(
      threads.find((c) => c.title === 'Wastewater')?.key,
    )
    expect(useStore.getState().conversation).not.toBe(useStore.getState().inspectedChat)

    const detail = container.querySelector('.detail-pane') ?? container.querySelector('.pane:last-child')
    const name = detail?.querySelector('.detail-title-edit') as HTMLInputElement | null
    expect(name?.value).toBe('Wastewater')
  })

  it('opens newest first, because that is the one you were just in', async () => {
    await show('chats', { conversations: [...threads].reverse() })

    expect(cells('.chats-grid.row')[0]).toContain('Screen time')
  })
})
