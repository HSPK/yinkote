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
import { DEFAULT_VISIBLE } from '../../lib/columns'
import { applyTheme, DEFAULT_THEME } from '../../lib/theme'
import type { State } from '../store'

export interface PrefsSlice {
  /** Pane widths in pixels; dragged by the splitters, persisted server-side. */
  layout: { sidebar: number; detail: number }
  /** Item-table column widths in pixels, keyed by column id. */
  columnWidths: Record<string, number>
  /** Visible columns, in display order. */
  columnOrder: string[]
  /** Whether the right-hand detail pane is showing. */
  detailOpen: boolean
  /** Row height preference, persisted server-side under `ui.`. */
  density: string
  theme: string
  /** Hex accent override, or empty to use the theme's own. */
  accent: string
  /** The style "copy citation" uses, remembered from the last one chosen. */
  citationStyle: string

  setLayout: (patch: Partial<{ sidebar: number; detail: number }>, commit?: boolean) => void
  setColumnWidth: (id: string, width: number, commit?: boolean) => void
  setColumnOrder: (order: string[]) => void
  resetColumns: () => void
  toggleDetail: (open?: boolean) => void
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
  columnOrder: DEFAULT_VISIBLE,
  detailOpen: true,
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
    if (commit) void api.settings.put({ columnWidths: JSON.stringify(columnWidths) })
  },

  setColumnOrder(columnOrder) {
    set({ columnOrder })
    void api.settings.put({ columnOrder: JSON.stringify(columnOrder) })
  },

  resetColumns() {
    set({ columnOrder: DEFAULT_VISIBLE, columnWidths: {} })
    void api.settings.put({ columnOrder: '', columnWidths: '' })
  },

  toggleDetail(open) {
    const detailOpen = open ?? !get().detailOpen
    set({ detailOpen })
    void api.settings.put({ detailOpen: String(detailOpen) })
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

    set({
      layout: parsed('ui.layout', get().layout),
      columnWidths: parsed('ui.columnWidths', {}),
      columnOrder: parsed('ui.columnOrder', DEFAULT_VISIBLE),
      detailOpen: text('ui.detailOpen') !== 'false',
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
