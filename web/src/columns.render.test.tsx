import { act, type ComponentType } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'

import { api } from './api/client'
import type { Collection, Conversation, Item } from './api/types'
import { ItemTable } from './components/ItemTable'
import { COLUMN_DRAG_TYPE } from './components/TableHeader'
import { useI18n, t } from './i18n'
import { CATALOGUE, DEFAULT_COLUMNS, restoredColumnOrders, type TableId } from './lib/columns'
import { beginDrag, dragging, readDrop } from './lib/dnd'
import { CollectionsPage } from './pages/CollectionsPage'
import { ChatsPage } from './pages/ChatsPage'
import { DownloadsPage } from './pages/DownloadsPage'
import { FilesPage } from './pages/FilesPage'
import { GapsPage } from './pages/GapsPage'
import { TasksPage } from './pages/TasksPage'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'
import { OverlayHost } from './ui/OverlayHost'
import { useOverlays } from './ui/overlays'

vi.mock('./api/client', () => ({
  api: {
    settings: { put: vi.fn().mockResolvedValue({}) },
    files: { list: vi.fn().mockResolvedValue({
      files: [{ key: 'file', filename: 'paper.pdf', parentKey: 'A', parentTitle: 'A paper', url: 'https://example.org/paper', bytes: 120 }],
      total: 1,
    }) },
    downloads: { list: vi.fn().mockResolvedValue({
      downloads: [{ id: 1, title: 'Download', url: 'https://example.org/paper', state: 'failed', error: 'notFound', bytes: 0 }],
      waiting: 0, failed: 1,
    }) },
    tasks: { list: vi.fn().mockResolvedValue({
      tasks: [{ id: 'task', kind: 'export', phase: 'running', done: 1, total: 2, message: '', startedAt: 100, result: null }],
    }) },
    references: { missing: vi.fn().mockResolvedValue({
      works: [{ fingerprint: 'missing', label: 'Missing work', doi: '10.1/example', year: 2020, citedBy: 3 }],
    }) },
  },
  connectEvents: () => () => {},
}))

beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as never
  Element.prototype.getBoundingClientRect = () => ({
    width: 1000, height: 800, top: 0, left: 0, right: 1000, bottom: 800, x: 0, y: 0,
    toJSON: () => ({}),
  })
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 800 })
})

const initial = useStore.getState()
let host: HTMLDivElement
let root: Root
const sort = vi.fn()

beforeEach(() => {
  vi.clearAllMocks()
  useI18n.getState().setLocale('en-US')
  useOverlays.getState().closeMenu()
  useStore.setState({
    ...initial,
    ...emptyScope({ items: [{
      key: 'A', title: 'A paper', itemType: 'journalArticle', creators: [], tags: [], collections: [],
    } as unknown as Item], total: 1 }),
    columnOrders: restoredColumnOrders({}),
    columnWidths: {},
    sort: 'title',
    direction: 'asc',
    setSort: sort,
    loadMore: vi.fn(),
    badgeDefs: [],
    badges: {},
    collections: [
      { key: 'A', name: 'Alpha', itemCount: 2, dateAdded: 100 } as Collection,
      { key: 'B', name: 'Beta', itemCount: 1, dateAdded: 200 } as Collection,
    ],
    smartCollections: [],
    conversations: [
      { key: 'A', title: 'Alpha', messageCount: 1, createdAt: 10, updatedAt: 20 } as Conversation,
      { key: 'B', title: 'Beta', messageCount: 2, createdAt: 20, updatedAt: 30 } as Conversation,
    ],
  })
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
})

afterEach(() => {
  act(() => root.unmount())
  useOverlays.getState().closeMenu()
  host.remove()
})

async function render(Component: ComponentType) {
  await act(async () => { root.render(<><Component /><OverlayHost /></>) })
}

function click(element: Element | null | undefined) {
  expect(element).toBeTruthy()
  act(() => { element!.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
}

function header(id?: string): HTMLElement {
  return host.querySelector<HTMLElement>(id ? `.table-head [data-column="${id}"]` : '.table-head')!
}

function columnIds() {
  return [...host.querySelectorAll<HTMLElement>('.table-head [data-column]')].map((c) => c.dataset.column)
}

function openColumns(id?: string) {
  act(() => { header(id).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 20, clientY: 20 })) })
  click(host.querySelector('[aria-haspopup="menu"]'))
}

function checkbox(label: string) {
  return [...host.querySelectorAll<HTMLButtonElement>('[role="menuitemcheckbox"]')]
    .find((button) => button.textContent?.replace('✓', '').trim() === label)!
}

function transfer() {
  const data = new Map<string, string>()
  return {
    effectAllowed: '', dropEffect: '',
    get types() { return [...data.keys()] },
    setData(type: string, value: string) { data.set(type, value) },
    getData(type: string) { return data.get(type) ?? '' },
  } as unknown as DataTransfer
}

function drag(element: Element, type: string, dataTransfer: DataTransfer) {
  const event = new Event(type, { bubbles: true, cancelable: true })
  Object.defineProperty(event, 'dataTransfer', { value: dataTransfer })
  act(() => { element.dispatchEvent(event) })
  return event
}

const pages: { table: TableId; Component: ComponentType; row: string; actions?: boolean }[] = [
  { table: 'items', Component: ItemTable, row: '.row' },
  { table: 'collections', Component: CollectionsPage, row: '.browser-grid.row', actions: true },
  { table: 'chats', Component: ChatsPage, row: '.chats-grid.row', actions: true },
  { table: 'files', Component: FilesPage, row: '.files-grid.row' },
  { table: 'downloads', Component: DownloadsPage, row: '.downloads-grid.row', actions: true },
  { table: 'tasks', Component: TasksPage, row: '.tasks-grid.row', actions: true },
  { table: 'gaps', Component: GapsPage, row: '.gaps-grid.row', actions: true },
]

describe('shared header columns', () => {
  it.each(pages)('$table uses live toggles and keeps rows, actions and widths aligned', async ({ table, Component, row, actions }) => {
    await render(Component)
    expect(columnIds()).toEqual(DEFAULT_COLUMNS[table])
    const rowBefore = host.querySelector(row)!
    const actionCount = rowBefore.querySelectorAll('button').length
    if (actions) expect(actionCount).toBeGreaterThan(0)
    const ids = DEFAULT_COLUMNS[table].slice(0, 2)
    openColumns()
    for (const id of ids) {
      const label = t(CATALOGUE[table].find((c) => c.id === id)!.labelKey)
      click(checkbox(label))
      expect(checkbox(label).getAttribute('aria-checked')).toBe('false')
    }
    expect(columnIds()).toEqual(DEFAULT_COLUMNS[table].filter((id) => !ids.includes(id)))
    expect(useStore.getState().columnOrders[table]).toEqual(columnIds())
    const drawnRow = host.querySelector<HTMLElement>(row)!
    expect(drawnRow.style.gridTemplateColumns).toBe(header().style.gridTemplateColumns)
    expect(drawnRow.querySelectorAll('button')).toHaveLength(actionCount)
    expect(drawnRow.querySelectorAll(':scope > .cell')).toHaveLength(columnIds().length + (actions ? 1 : 0))
    expect(Number.parseFloat(header().style.minWidth)).toBeGreaterThan(0)
    expect(host.querySelector('.submenu')).not.toBeNull()
  })

  it('refreshes plugin checks and labels while keeping every handler live', async () => {
    const badges = [
      { id: 'if', label: 'Impact', pluginId: 'metrics', sortable: true, needs: [] },
      { id: 'rank', label: 'Rank', pluginId: 'metrics', needs: [] },
    ]
    useStore.setState({ badgeDefs: badges, badges: { A: [
      { badge: 'if', pluginId: 'metrics', text: '12.3' },
      { badge: 'rank', pluginId: 'metrics', text: 'Q1' },
    ] } })
    await render(ItemTable)
    openColumns('title')
    const oldHandler = useOverlays.getState().menu!.items[0]!.items!
      .find((item) => item.id === 'badge:metrics:rank')!.onSelect!
    click(checkbox('Impact'))
    act(() => { void oldHandler() })
    expect(columnIds()).toEqual(expect.arrayContaining(['badge:metrics:if', 'badge:metrics:rank']))
    expect(checkbox('Impact').getAttribute('aria-checked')).toBe('true')
    expect(checkbox('Rank').getAttribute('aria-checked')).toBe('true')
    expect(host.querySelector('.row')?.textContent).toContain('12.3')
    expect(host.querySelector('.row')?.textContent).toContain('Q1')
    act(() => {
      useStore.setState({ badgeDefs: badges.map((badge) => ({ ...badge, label: `${badge.label}!` })) })
      useI18n.getState().setLocale('zh-CN')
    })
    expect(checkbox('Impact!').getAttribute('aria-checked')).toBe('true')
    expect(host.querySelector('[aria-haspopup="menu"]')?.textContent).toContain('列')
    click(checkbox('Rank!'))
    expect(columnIds()).not.toContain('badge:metrics:rank')
    expect(columnIds()).toContain('badge:metrics:if')
    act(() => useOverlays.getState().closeMenu())
    click(header('badge:metrics:if').querySelector('button'))
    expect(sort).toHaveBeenCalledWith('badge:metrics:if')
  })

  it('retains a visible column even when the saved order only contains missing plugins', async () => {
    useStore.setState({ columnOrders: { ...DEFAULT_COLUMNS, items: ['badge:disabled:metric'] } })
    await render(ItemTable)
    expect(columnIds()).toEqual(['title'])
    openColumns('title')
    expect(checkbox('Title').disabled).toBe(true)
    click(checkbox('Year'))
    expect(columnIds()).toContain('year')
    click(checkbox('Title'))
    expect(columnIds()).toEqual(['year'])
    expect(checkbox('Year').disabled).toBe(true)
  })

  it('re-enables columns without resetting a manually arranged visible order', async () => {
    useStore.setState({ columnOrders: { ...DEFAULT_COLUMNS, items: ['modified', 'year', 'title'] } })
    await render(ItemTable)
    openColumns()
    click(checkbox('Author'))
    expect(columnIds().filter((id) => id !== 'author')).toEqual(['modified', 'year', 'title'])
    click(checkbox('Tags'))
    expect(columnIds().filter((id) => !['author', 'tags'].includes(id!))).toEqual(['modified', 'year', 'title'])
  })

  it.each(pages)('$table drags only columns, persisting its own order without sorting', async ({ table, Component, row }) => {
    await render(Component)
    const order = [...DEFAULT_COLUMNS[table]]
    const beforeRows = [...host.querySelectorAll(row)].map((row) => row.textContent)
    const source = header(order[0])
    const target = header(order[order.length - 1])
    const data = transfer()
    drag(source, 'dragstart', data)
    expect(data.types).toEqual([COLUMN_DRAG_TYPE])
    expect(dragging()).toBeNull()
    expect(readDrop({ dataTransfer: data } as React.DragEvent)).toBeNull()
    drag(target, 'dragover', data)
    drag(target, 'drop', data)
    drag(source, 'dragend', data)
    click(target.querySelector('button'))
    const expected = [...order.slice(1), order[0]]
    expect(columnIds()).toEqual(expected)
    expect(useStore.getState().columnOrders[table]).toEqual(expected)
    expect(sort).not.toHaveBeenCalled()
    expect(host.querySelectorAll(row)).toHaveLength(beforeRows.length)
    const persisted = JSON.parse(vi.mocked(api.settings.put).mock.calls.at(-1)![0].columnOrders as string)
    expect(persisted[table]).toEqual(expected)
    for (const other of Object.keys(DEFAULT_COLUMNS) as TableId[]) {
      if (other !== table) expect(persisted[other]).toEqual(DEFAULT_COLUMNS[other])
    }
    act(() => { root.render(<OverlayHost />) })
    await render(Component)
    expect(columnIds()).toEqual(expected)
  })

  it('ignores item/file drops and cross-table column payloads', async () => {
    await render(ItemTable)
    const data = transfer()
    beginDrag({ dataTransfer: data } as React.DragEvent, { kind: 'items', keys: ['A'] }, 'A')
    drag(header('year'), 'drop', data)
    expect(columnIds()).toEqual(DEFAULT_COLUMNS.items)
    data.setData(COLUMN_DRAG_TYPE, JSON.stringify({ table: 'collections', id: 'name' }))
    drag(header('year'), 'drop', data)
    expect(columnIds()).toEqual(DEFAULT_COLUMNS.items)
    const columnData = transfer()
    drag(header('title'), 'dragstart', columnData)
    expect(dragging()).toBeNull()
    expect(readDrop({ dataTransfer: columnData } as React.DragEvent)).toBeNull()
    drag(header('title'), 'dragend', columnData)
  })

  it('keeps collection sorting and the active arrow through a reorder', async () => {
    await render(CollectionsPage)
    click(header('items').querySelector('button'))
    expect(host.querySelector('.row')?.textContent).toContain('Beta')
    const data = transfer()
    drag(header('items'), 'dragstart', data)
    drag(header('name'), 'drop', data)
    drag(header('items'), 'dragend', data)
    expect(host.querySelector('.row')?.textContent).toContain('Beta')
    expect(header('items').getAttribute('aria-sort')).toBe('ascending')
    expect(header('items').querySelector('.sort-arrow')?.textContent).toBe('↑')
  })

  it('supports keyboard reordering and opening the columns submenu', async () => {
    await render(ChatsPage)
    act(() => { header('messages').dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'ArrowLeft', altKey: true })) })
    expect(columnIds()[0]).toBe('messages')
    act(() => { header('messages').dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'F10', shiftKey: true })) })
    const submenu = host.querySelector('[aria-haspopup="menu"]')!
    act(() => { submenu.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'Enter' })) })
    expect(host.querySelector('.submenu')).not.toBeNull()
  })

  it('keeps keyboard focus through checkbox and locale refreshes, then restores it', async () => {
    await render(ChatsPage)
    vi.useFakeTimers()
    try {
      header('title').focus()
      act(() => { header('title').dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'ContextMenu' })) })
      await act(async () => { await vi.advanceTimersByTimeAsync(30) })
      const parent = host.querySelector('[aria-haspopup="menu"]')!
      expect(document.activeElement).toBe(parent)
      act(() => { parent.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'ArrowRight' })) })
      await act(async () => { await vi.advanceTimersByTimeAsync(30) })
      const first = host.querySelector('[role="menuitemcheckbox"]')!
      expect(document.activeElement).toBe(first)
      act(() => { first.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'ArrowDown' })) })
      const second = document.activeElement!
      expect(second).not.toBe(first)
      click(second)
      act(() => useI18n.getState().setLocale('zh-CN'))
      expect(document.activeElement).toBe(second)
      expect(second.getAttribute('aria-checked')).toBe('false')
      act(() => { second.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'Escape' })) })
      expect(useOverlays.getState().menu).toBeNull()
      expect(document.activeElement).toBe(header('title'))
    } finally {
      vi.useRealTimers()
    }
  })

  it('keeps resized widths attached to columns across dragging without sorting', async () => {
    await render(ItemTable)
    header('title').getBoundingClientRect = () => ({ width: 210 }) as DOMRect
    act(() => {
      header('title').querySelector('.col-grip')!.dispatchEvent(
        new MouseEvent('pointerdown', { bubbles: true, clientX: 100 }),
      )
      window.dispatchEvent(new MouseEvent('pointermove', { clientX: 150 }))
      window.dispatchEvent(new MouseEvent('pointerup', { clientX: 150 }))
    })
    expect(useStore.getState().columnWidths.title).toBe(260)
    const data = transfer()
    drag(header('title'), 'dragstart', data)
    drag(header('year'), 'drop', data)
    drag(header('title'), 'dragend', data)
    expect(header().style.gridTemplateColumns).toContain('260px')
    expect(useStore.getState().columnWidths.title).toBe(260)
    expect(sort).not.toHaveBeenCalled()
    expect(document.body.style.cursor).toBe('')
    expect(vi.mocked(api.settings.put).mock.calls.some(([patch]) =>
      patch.columnWidths && JSON.parse(patch.columnWidths as string).title === 260,
    )).toBe(true)
  })

  it('resets all widths for only the chosen table, including hidden columns', async () => {
    useStore.setState({
      columnOrders: { ...DEFAULT_COLUMNS, collections: ['items', 'name'] },
      columnWidths: { title: 300, 'collections:name': 350, 'collections:modified': 150, 'chats:title': 240 },
    })
    await render(CollectionsPage)
    openColumns()
    click([...host.querySelectorAll('.submenu button')].find((button) => button.textContent === t('table.resetColumns')))
    expect(columnIds()).toEqual(DEFAULT_COLUMNS.collections)
    expect(useStore.getState().columnWidths).toEqual({
      title: 300, 'chats:title': 240,
      ...Object.fromEntries(CATALOGUE.collections.map((c) => [`collections:${c.id}`, c.width])),
    })
    expect(useOverlays.getState().menu).toBeNull()
  })

  it('migrates old per-table preferences and widths', () => {
    act(() => { useStore.getState().restorePrefs({
      'ui.columnOrder': '["year","title"]',
      'ui.columnOrders': '{"collections":["items","name"]}',
      'ui.columnWidths': '{"title":300,"name":240}',
    }) })
    expect(useStore.getState().columnOrders).toEqual({
      ...DEFAULT_COLUMNS, items: ['year', 'title'], collections: ['items', 'name'],
    })
    expect(useStore.getState().columnWidths).toMatchObject({
      title: 300, 'chats:title': 300, 'collections:name': 240,
    })
  })

  it('does not resurrect migrated widths after resetting and reloading a table', () => {
    act(() => {
      useStore.getState().restorePrefs({ 'ui.columnWidths': '{"title":300,"name":240}' })
      useStore.getState().resetColumns('chats')
      useStore.getState().resetColumns('collections')
    })
    const saved = { ...vi.mocked(api.settings.put).mock.calls.at(-1)![0] }
    act(() => { useStore.getState().restorePrefs({
      'ui.columnOrders': saved.columnOrders,
      'ui.columnWidths': saved.columnWidths,
    }) })
    expect(useStore.getState().columnWidths.title).toBe(300)
    expect(useStore.getState().columnWidths['chats:title']).toBe(0)
    expect(useStore.getState().columnWidths['collections:name']).toBe(0)
  })
})
