/** Presentation preferences.
 *
 *  Everything here is about how the workbench looks rather than what it holds,
 *  and every one of them is written back to the server so a setting follows the
 *  user between browsers. Split out because "how do I look" and "what am I
 *  showing" change for entirely different reasons.
 */
import type { StateCreator } from 'zustand'

import { api } from '../../api/client'
import { detectLocale, useI18n, type Locale } from '../../i18n'
import { CATALOGUE, DEFAULT_COLUMNS, columnWidthKey, restoredColumnOrders, type TableId } from '../../lib/columns'
import { DEFAULT_READER_LAYOUT, readerLayoutFrom, type ReaderLayout } from '../../lib/reader-layout'
import { failureOf } from '../../lib/errors'
import { applyTheme, DEFAULT_THEME } from '../../lib/theme'
import type { State } from '../store'

export interface PrefsSlice {
  /** Pane widths in pixels; dragged by the splitters, persisted server-side. */
  layout: { sidebar: number; detail: number }
  /** Column widths; non-item tables namespace their column ids. */
  columnWidths: Record<string, number>
  /** Visible columns per table, in display order. */
  columnOrders: Record<TableId, string[]>
  /** Whether the right-hand detail pane is showing. */
  detailOpen: boolean
  sidebarOpen: boolean
  readerLayout: ReaderLayout
  /** Row height preference, persisted server-side under `ui.`. */
  density: string
  theme: string
  /** Hex accent override, or empty to use the theme's own. */
  accent: string
  /** The style "copy citation" uses, remembered from the last one chosen. */
  citationStyle: string

  setLayout: (patch: Partial<{ sidebar: number; detail: number }>, commit?: boolean) => void
  setColumnWidth: (id: string, width: number, commit?: boolean) => void
  setColumnOrder: (table: TableId, order: string[]) => void
  resetColumns: (table: TableId) => void
  toggleDetail: (open?: boolean) => void
  toggleSidebar: () => void
  setReaderLayout: (patch: Partial<ReaderLayout>, commit?: boolean) => void
  setDensity: (d: string) => void
  setTheme: (id: string, accent?: string) => void
  setCitationStyle: (id: string) => void
  setLocale: (locale: Locale) => void
  /** Apply what was saved server-side. Called once, during bootstrap. */
  restorePrefs: (settings: Record<string, unknown>) => void
}

export const createPrefsSlice: StateCreator<State, [], [], PrefsSlice> = (set, get) => ({
  layout: { sidebar: 232, detail: 380 },
  columnWidths: {},
  columnOrders: { ...DEFAULT_COLUMNS },
  detailOpen: true,
  sidebarOpen: true,
  readerLayout: DEFAULT_READER_LAYOUT,
  density: 'compact',
  theme: DEFAULT_THEME,
  accent: '',
  citationStyle: 'apa',

  setLayout(patch, commit) {
    const layout = { ...get().layout, ...patch }
    set({ layout })
    // Only persist when the drag ends; a write per mouse move is pointless.
    if (commit) void api.settings.put({ layout: JSON.stringify(layout) })
  },

  setColumnWidth(id, width, commit) {
    const columnWidths = { ...get().columnWidths, [id]: width }
    set({ columnWidths })
    if (commit) {
      void api.settings.put({ columnWidths: JSON.stringify(columnWidths) })
        .catch((error: unknown) => set({ error: failureOf(error) }))
    }
  },

  setColumnOrder(table, order) {
    if (!order.length) return
    const columnOrders = { ...get().columnOrders, [table]: [...new Set(order)] }
    set({ columnOrders })
    void api.settings.put({ columnOrders: JSON.stringify(columnOrders) })
      .catch((error: unknown) => set({ error: failureOf(error) }))
  },

  resetColumns(table) {
    const columnOrders = { ...get().columnOrders, [table]: [...DEFAULT_COLUMNS[table]] }
    const columnWidths = { ...get().columnWidths }
    for (const id of Object.keys(columnWidths)) {
      if (table === 'items'
        ? CATALOGUE.items.some((c) => c.id === id) || id.startsWith('badge:')
        : id.startsWith(`${table}:`)) delete columnWidths[id]
    }
    // Explicit defaults prevent legacy shared widths from being migrated back
    // into a table after its next reload.
    if (table !== 'items') {
      for (const column of CATALOGUE[table]) {
        columnWidths[columnWidthKey(table, column.id)] = column.width
      }
    }
    set({ columnOrders, columnWidths })
    void api.settings.put({
      columnOrders: JSON.stringify(columnOrders),
      columnWidths: JSON.stringify(columnWidths),
    }).catch((error: unknown) => set({ error: failureOf(error) }))
  },

  toggleDetail(open) {
    if (get().tabs.find((tab) => tab.id === get().activeTab)?.kind === 'reader') {
      get().setReaderLayout({ detailsOpen: open ?? !get().readerLayout.detailsOpen })
      return
    }
    const detailOpen = open ?? !get().detailOpen
    set({ detailOpen })
    void api.settings.put({ detailOpen: String(detailOpen) })
  },

  toggleSidebar() {
    const sidebarOpen = !get().sidebarOpen
    set({ sidebarOpen })
    void api.settings.put({ sidebarOpen: String(sidebarOpen) })
      .catch((error: unknown) => set({ error: failureOf(error) }))
  },

  setReaderLayout(patch, commit = true) {
    const readerLayout = readerLayoutFrom({ ...get().readerLayout, ...patch })
    set({ readerLayout })
    if (commit) {
      void api.settings.put({ readerLayout: JSON.stringify(readerLayout) })
        .catch((error: unknown) => set({ error: failureOf(error) }))
    }
  },

  setDensity(density) {
    set({ density })
    document.documentElement.style.setProperty(
      '--row-h',
      density === 'comfortable' ? '32px' : '26px',
    )
    void api.settings.put({ density })
  },

  setTheme(theme, accent) {
    const next = accent ?? get().accent
    set({ theme, accent: next })
    applyTheme(theme, next)
    void api.settings.put({ theme, accent: next })
  },

  /** Read every preference out of the settings blob.
   *
   *  Lives here rather than in bootstrap so that adding a preference means
   *  touching one file: its field, its setter and its restore, all in view of
   *  each other. Anything unreadable falls back rather than throwing — a
   *  corrupt setting must not stop the workbench opening. */
  restorePrefs(settings) {
    const text = <T extends string>(key: string): T | undefined =>
      typeof settings[key] === 'string' ? (settings[key] as T) : undefined

    const parsed = <T,>(key: string, fallback: T): T => {
      const raw = text(key)
      if (!raw) return fallback
      try {
        return JSON.parse(raw) as T
      } catch {
        return fallback
      }
    }

    if (text('ui.density')) get().setDensity(text('ui.density')!)

    const savedWidths = parsed<unknown>('ui.columnWidths', {})
    const columnWidths = Object.fromEntries(
      Object.entries(savedWidths && typeof savedWidths === 'object' ? savedWidths : {})
        .filter((entry): entry is [string, number] =>
          typeof entry[1] === 'number' && Number.isFinite(entry[1]) && entry[1] >= 0),
    )
    // Earlier collection/chat tables shared the item width map. Copy their
    // saved widths once, so subsequent resizing and resetting are independent.
    for (const table of ['collections', 'chats'] as const) {
      for (const column of CATALOGUE[table]) {
        const key = columnWidthKey(table, column.id)
        if (columnWidths[key] === undefined && columnWidths[column.id] !== undefined) {
          columnWidths[key] = columnWidths[column.id]!
        }
      }
    }

    set({
      layout: parsed('ui.layout', get().layout),
      columnWidths,
      // `ui.columnOrder` is what single-table installs saved. It is still read
      // as the item table's order so that upgrading does not silently throw
      // away a layout somebody arranged.
      columnOrders: restoredColumnOrders(
        parsed<unknown>('ui.columnOrders', {}),
        parsed<unknown>('ui.columnOrder', DEFAULT_COLUMNS.items),
      ),
      detailOpen: text('ui.detailOpen') !== 'false',
      sidebarOpen: text('ui.sidebarOpen') !== 'false',
      readerLayout: readerLayoutFrom(parsed<unknown>('ui.readerLayout', {})),
      citationStyle: text('ui.citationStyle') ?? 'apa',
    })

    const theme = text('ui.theme') ?? DEFAULT_THEME
    const accent = text('ui.accent') ?? ''
    set({ theme, accent })
    applyTheme(theme, accent)

    useI18n.getState().setLocale(text<Locale>('ui.locale') ?? detectLocale())
  },

  setCitationStyle(citationStyle) {
    set({ citationStyle })
    void api.settings.put({ citationStyle })
  },

  setLocale(locale) {
    useI18n.getState().setLocale(locale)
    void api.settings.put({ locale })
  },
})
