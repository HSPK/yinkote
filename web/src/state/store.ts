/** Application state.
 *
 *  A single store: this is one screen with tightly coupled panes, and splitting
 *  it would only add ceremony. Server data and UI data are kept in separate
 *  slices so it stays obvious which is which.
 */
import { create } from 'zustand'

import { api, connectEvents } from '../api/client'
import { schemaLabel, t, useI18n } from '../i18n'
import { createPrefsSlice, type PrefsSlice } from './slices/prefs'
import { createChatSlice, type ChatSlice } from './slices/chat'
import { createSidebarSlice, type SidebarSlice } from './slices/sidebar'
import {
  applyClick,
  captureScope,
  emptyScope,
  type Scope,
} from './scope'
import {
  LIBRARY_TAB_ID,
  closeAll,
  keepTab,
  closeOthers,
  closeTab,
  libraryTab,
  nextActive,
  openTab,
  tabId,
  type Tab,
  type TabKind,
} from '../lib/tabs'
import { inferMode } from '../lib/query'
import { useOverlays } from '../ui/overlays'
import type {
  BadgeDescriptor,
  BadgeValue,
  ListQuery,
  PluginStatus,
  Schema,
  ServerInfo,
  Stats,
} from '../api/types'

/** Secondary surfaces. `null` means the workbench itself is in front. */
/** Only preferences remain modal: everything you *work in* is a tab. */
export type Modal = null | 'settings'
export type Panel = 'detail' | 'plugins' | 'stats'

export interface State extends Scope, PrefsSlice, SidebarSlice, ChatSlice {
  // connection & metadata
  ready: boolean
  connected: boolean
  error: string | null
  library: number
  schema: Schema | null
  stats: Stats | null
  server: ServerInfo | null
  plugins: PluginStatus[]

  /** Which secondary surface is open, if any. */
  modal: Modal
  /** Open workspace tabs and the one in front. */
  tabs: Tab[]
  activeTab: string
  /** Saved library state for tabs that are not in front. */
  scopes: Record<string, Scope>
  /** The collection being edited: its key, `'new'`, or closed. */
  collectionEditor: string | null
  /** What the toolbar's search box holds for surfaces that are not the library.
   *  Kept apart from `query` so switching tabs does not run one as the other. */
  filter: string
  /** Badge columns offered by plugins. */
  badgeDefs: BadgeDescriptor[]
  /** Resolved badges for the rows currently loaded, keyed by item key. */
  badges: Record<string, BadgeValue[]>


  // The active tab's library state is projected onto the store, so components
  // read `items` and `query` without knowing which tab they belong to.


  panel: Panel
  paletteOpen: boolean

  bootstrap: () => Promise<void>
  refresh: () => Promise<void>
  loadMore: () => Promise<void>
  listQuery: (offset?: number) => ListQuery
  setQuery: (q: string) => void
  setSort: (field: string) => void
  navigate: (patch: Partial<State>) => void
  openLibrary: () => void
  openTrash: () => void
  openCollection: (key: string) => void
  openSmart: (key: string) => void
  clearFilters: () => void
  select: (key: string, modifier?: 'none' | 'toggle' | 'range') => void
  selectAll: () => void
  moveCursor: (delta: number) => void
  setPanel: (p: Panel) => void
  setModal: (m: Modal) => void
  setFilter: (value: string) => void
  openTab: (tab: Tab) => void
  closeTab: (id: string) => void
  closeTabs: (scope: 'all' | 'others', keep?: string) => void
  activateTab: (id: string) => void
  keepTab: (id: string) => void
  openReader: (itemKey: string, keep?: boolean) => void
  fetchPdf: (itemKey: string, url?: string) => Promise<void>
  openCollectionEditor: (key: string | null) => void
  loadBadges: (keys: string[]) => Promise<void>
  optimize: () => Promise<void>
  togglePalette: (open?: boolean) => void
  patchItem: (key: string, patch: Record<string, unknown>) => Promise<void>
  trashSelected: () => Promise<void>
  restoreSelected: () => Promise<void>
  destroySelected: () => Promise<void>
  emptyTrash: () => Promise<void>
  createItem: (itemType: string, title: string) => Promise<void>
  /** Ask for a type and title. Resolves to `null` when cancelled. */
  newItemDialog: () => Promise<{ itemType: string; title: string } | null>
  trashItems: (keys: string[]) => Promise<void>
  copySelected: (kind: 'title' | 'doi' | 'url' | 'citation') => Promise<number>
  setPluginEnabled: (id: string, enabled: boolean) => Promise<void>
  reloadPlugins: () => Promise<void>
  reindex: () => Promise<void>
}

const PAGE = 200

/** Tab kinds whose content is a list of items, and so own a library scope. */
const TAB_OWNS_LIBRARY = new Set<TabKind>(['library'])

/** The scope a freshly shown tab starts with. */
function scopeFor(tab: Tab): Partial<Scope> {
  return tab.target ? { view: 'collection', collection: tab.target } : {}
}
/** Long enough to avoid a request per keystroke, short enough to feel live. */
const DEBOUNCE_MS = 140

let debounce: number | undefined
let requestSeq = 0

export const useStore = create<State>((set, get, store) => ({
  ...createPrefsSlice(set, get, store),
  ...createSidebarSlice(set, get, store),
  ...createChatSlice(set, get, store),

  ready: false,
  connected: false,
  error: null,
  library: 1,
  schema: null,
  stats: null,
  server: null,
  plugins: [],
  modal: null,
  tabs: [libraryTab('')],
  activeTab: LIBRARY_TAB_ID,
  scopes: {},
  collectionEditor: null,
  filter: '',
  badgeDefs: [],
  badges: {},

  // The library fields come from one place, so a new one cannot be forgotten
  // here or in `captureScope`.
  ...emptyScope(),


  panel: 'detail',
  paletteOpen: false,

  async bootstrap() {
    try {
      const server = await api.ping()
      const [schema, collections, settings] = await Promise.all([
        api.schema(),
        api.collections.list(server.defaultLibrary),
        api.settings.get().catch(() => ({}) as Record<string, unknown>),
      ])
      set({
        library: server.defaultLibrary,
        server,
        schema,
        collections,
        ready: true,
        error: null,
      })
      // Restore preferences before the first paint of real content.
      get().restorePrefs(settings)
      await Promise.all([get().refresh(), get().reloadSidebar()])

      connectEvents((event) => {
        const type = event.type as string
        if (type === 'connected') set({ connected: true })
        else if (type === 'disconnected') set({ connected: false })
        else if (type.startsWith('items') || type === 'collectionsChanged' || type === 'tagsChanged') {
          // The server pushes only that *something* changed; refetching the
          // current page is cheap and always correct.
          void get().refresh()
          void get().reloadSidebar()
        } else if (type === 'pluginsChanged') {
          // Badge columns come and go with their plugin, and any answers the
          // old one gave are no longer trustworthy.
          set({ badges: {} })
          void get().reloadSidebar()
        }
      })
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e), ready: true })
    }
  },

  /** Build the list query for the current view; shared by the first page and
   *  every one after it, so they cannot disagree about what is being listed. */
  listQuery(offset = 0) {
    const s = get()
    return {
      q: s.query || undefined,
      mode: s.query ? s.mode : undefined,
      collection: s.view === 'collection' ? (s.collection ?? undefined) : undefined,
      tag: s.activeTags.length ? s.activeTags : undefined,
      itemType: s.typeFilter.length ? s.typeFilter : undefined,
      trash: s.view === 'trash' ? 'only' : 'exclude',
      sort: s.sort,
      direction: s.direction,
      limit: PAGE,
      offset: offset || undefined,
    } satisfies ListQuery
  },

  async refresh() {
    const s = get()
    const seq = ++requestSeq
    set({ loading: true })
    const started = performance.now()
    try {
      const page = await api.items.list(s.library, get().listQuery())
      // Ignore responses that a newer keystroke has already superseded.
      if (seq !== requestSeq) return
      set({
        items: page.items,
        total: page.total,
        loading: false,
        tookMs: Math.round(performance.now() - started),
        cursor: Math.min(get().cursor, Math.max(0, page.items.length - 1)),
        error: null,
      })
      void get().loadBadges(page.items.map((i) => i.key))
    } catch (e) {
      if (seq !== requestSeq) return
      set({ loading: false, error: e instanceof Error ? e.message : String(e) })
    }
  },

  /** Append the next page.
   *
   *  Guarded by the same sequence number as `refresh`, so a page that arrives
   *  after the view has changed is dropped instead of being stitched onto a
   *  list it does not belong to. */
  async loadMore() {
    const s = get()
    if (s.loadingMore || s.loading || s.items.length >= s.total) return
    const seq = requestSeq
    set({ loadingMore: true })
    try {
      const page = await api.items.list(s.library, get().listQuery(s.items.length))
      if (seq !== requestSeq) return
      set({ items: [...get().items, ...page.items], total: page.total, loadingMore: false })
      void get().loadBadges(page.items.map((i) => i.key))
    } catch {
      if (seq === requestSeq) set({ loadingMore: false })
    }
  },


  /** Setting a query also re-decides how to run it.
   *
   *  The mode is a consequence of what was typed, not a separate choice: a
   *  quoted phrase wants exact matching, a bare filter wants a lookup, and
   *  prose wants everything fused. Asking the user to pick was asking them to
   *  know our retrieval pipeline. */
  setQuery(q) {
    set({ query: q, mode: inferMode(q) })
    if (debounce) window.clearTimeout(debounce)
    debounce = window.setTimeout(() => void get().refresh(), DEBOUNCE_MS)
  },

  setSort(field) {
    const s = get()
    const direction = s.sort === field && s.direction === 'desc' ? 'asc' : 'desc'
    set({ sort: field, direction })
    void get().refresh()
  },

  /** Move to a view, discarding whatever the previous one owned.
   *
   *  A smart collection sets the query, mode and sort — they *are* the smart
   *  collection. Leaving it must put them back, or the next view silently
   *  inherits a filter the user never typed and cannot see the origin of. */
  navigate(patch) {
    // A fresh scope, then whatever the destination asked for: one definition of
    // "clean slate", shared with a newly shown tab.
    set({ ...emptyScope(), items: get().items, ...patch })
    void get().refresh()
    void get().reloadSidebar()
  },

  openLibrary() {
    get().navigate({ view: 'library', collection: null })
  },

  openTrash() {
    get().navigate({ view: 'trash', collection: null })
  },

  openCollection(key) {
    get().navigate({ view: 'collection', collection: key })
  },

  /** Opening a smart collection *is* running its query: the search box shows
   *  exactly what is being matched, so the result is never mysterious. */
  openSmart(key) {
    const smart = get().smartCollections.find((s) => s.key === key)
    if (!smart) return
    get().navigate({
      view: 'smart',
      collection: key,
      query: smart.query,
      mode: smart.mode,
      sort: smart.sort,
      direction: smart.direction,
    })
  },





  clearFilters() {
    set({ activeTags: [], typeFilter: [], query: '' })
    void get().refresh()
  },

  select(key, modifier = 'none') {
    const s = get()
    const keys = s.items.map((i) => i.key)
    const index = keys.indexOf(key)
    if (index < 0) return
    set({ ...applyClick(keys, s, index, modifier), panel: 'detail' })
  },

  selectAll() {
    const s = get()
    set({ selected: s.items.map((i) => i.key), anchor: 0, cursor: s.items.length - 1 })
  },

  moveCursor(delta) {
    const s = get()
    if (!s.items.length) return
    const cursor = Math.max(0, Math.min(s.items.length - 1, s.cursor + delta))
    const item = s.items[cursor]
    set({ cursor, selected: item ? [item.key] : s.selected })
  },

  setPanel(panel) {
    set({ panel })
  },

  setModal(modal) {
    set({ modal })
  },

  setFilter(filter) {
    set({ filter })
  },

  openTab(tab) {
    const s = get()
    if (tab.id === s.activeTab) return set({ tabs: openTab(s.tabs, tab) })
    // Capture before the switch, then let `activateTab` restore.
    set({ tabs: openTab(s.tabs, tab), scopes: { ...s.scopes, [s.activeTab]: captureScope(s) } })
    const previous = get().activeTab
    set({ activeTab: previous })
    get().activateTab(tab.id)
  },

  closeTab(id) {
    const s = get()
    const next = nextActive(s.tabs, id, s.activeTab)
    // Forget the closed tab's scope; keeping it would leak a list of items per
    // tab ever opened.
    const scopes = { ...s.scopes }
    delete scopes[id]
    set({ tabs: closeTab(s.tabs, id), scopes })
    if (next !== s.activeTab) {
      set({ activeTab: s.activeTab })
      get().activateTab(next)
    }
  },

  closeTabs(scope, keep) {
    const tabs = scope === 'all' ? closeAll(get().tabs) : closeOthers(get().tabs, keep ?? '')
    set({ tabs, activeTab: tabs.some((t) => t.id === get().activeTab) ? get().activeTab : LIBRARY_TAB_ID })
  },

  /** Switch tabs, saving what the old one owned and restoring the new one's.
   *
   *  The only place scopes are captured or applied: a scope that is sometimes
   *  saved would be worse than having none. */
  activateTab(activeTab) {
    const s = get()
    if (activeTab === s.activeTab) return

    const scopes = { ...s.scopes, [s.activeTab]: captureScope(s) }
    const restored = scopes[activeTab]
    set({ activeTab, scopes, ...(restored ?? {}) })

    // A library tab that has never been shown has nothing to restore, so it
    // fetches once rather than sitting empty.
    const tab = s.tabs.find((t) => t.id === activeTab)
    if (!restored && tab && TAB_OWNS_LIBRARY.has(tab.kind)) {
      set(emptyScope(scopeFor(tab)))
      void get().refresh()
    }
  },

  keepTab(id) {
    set({ tabs: keepTab(get().tabs, id) })
  },

  /** Show a paper. A preview by default, so skimming a list of results does
   *  not leave a tab behind for every one glanced at. */
  openReader(itemKey, keep = false) {
    const title = get().items.find((i) => i.key === itemKey)?.title
    get().openTab({
      id: tabId('reader', itemKey),
      kind: 'reader',
      title: String(title ?? itemKey),
      target: itemKey,
      preview: !keep,
    })
  },



  /** Download the item's PDF and attach it, then show it. */
  async fetchPdf(itemKey, url) {
    const result = await api.files.fetch(get().library, itemKey, url)
    await get().refresh()
    get().openReader(itemKey)
    return void result
  },

  openCollectionEditor(collectionEditor) {
    set({ collectionEditor })
  },






  /** Fill in plugin badges for rows that do not have them yet.
   *
   *  Deliberately fire-and-forget: the table is already on screen, and a slow
   *  or broken badge plugin must never be able to hold a listing back. */
  async loadBadges(keys) {
    const s = get()
    if (!s.badgeDefs.length) return
    const wanted = keys.filter((k) => !(k in s.badges))
    if (!wanted.length) return
    try {
      const resolved = await api.badges.resolve(s.library, wanted)
      // Remember misses too, so a blank cell is asked about once, not forever.
      const merged = { ...get().badges }
      for (const k of wanted) merged[k] = resolved[k] ?? []
      set({ badges: merged })
    } catch {
      // A badge is an extra, never a reason to surface an error.
    }
  },










  async optimize() {
    await api.maintenance.optimize()
  },

  togglePalette(open) {
    set({ paletteOpen: open ?? !get().paletteOpen })
  },

  async patchItem(key, patch) {
    const s = get()
    const current = s.items.find((i) => i.key === key)
    try {
      const updated = await api.items.update(s.library, key, patch, current?.version)
      set({ items: s.items.map((i) => (i.key === key ? { ...i, ...updated } : i)) })
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) })
      await get().refresh()
    }
  },

  async trashSelected() {
    const s = get()
    if (!s.selected.length) return
    await api.items.trash(s.library, s.selected)
    set({ selected: [] })
    await Promise.all([get().refresh(), get().reloadSidebar()])
  },

  async restoreSelected() {
    const s = get()
    if (!s.selected.length) return
    await api.items.restore(s.library, s.selected)
    set({ selected: [] })
    await Promise.all([get().refresh(), get().reloadSidebar()])
  },

  async destroySelected() {
    const s = get()
    if (!s.selected.length) return
    await api.items.destroy(s.library, s.selected)
    set({ selected: [] })
    await Promise.all([get().refresh(), get().reloadSidebar()])
  },

  async emptyTrash() {
    await api.items.emptyTrash(get().library)
    set({ selected: [] })
    await Promise.all([get().refresh(), get().reloadSidebar()])
  },

  async createItem(itemType, title) {
    const s = get()
    const draft: Record<string, unknown> = { itemType, title }
    if (s.view === 'collection' && s.collection) draft.collections = [s.collection]
    const res = await api.items.create(s.library, [draft])
    await Promise.all([get().refresh(), get().reloadSidebar()])
    const created = res.created[0]
    if (created) set({ selected: [created.key], panel: 'detail' })
  },

  async newItemDialog() {
    const types = (get().schema?.itemTypes ?? []).filter((t) => !t.internal)
    const values = await useOverlays.getState().ask({
      title: t('dialog.newItem'),
      fields: [
        {
          name: 'title',
          label: t('table.title'),
          required: true,
          autoFocus: true,
          placeholder: t('dialog.itemTitle'),
        },
        {
          name: 'itemType',
          label: t('table.type'),
          type: 'select',
          defaultValue: 'journalArticle',
          options: types.map((d) => ({
            value: d.type,
            label: schemaLabel(d, useI18n.getState().locale, d.type),
          })),
        },
      ],
      confirmLabel: t('dialog.create'),
    })
    if (!values?.title?.trim()) return null
    return {
      title: values.title.trim(),
      itemType: values.itemType || 'journalArticle',
    }
  },








  async trashItems(keys) {
    if (!keys.length) return
    await api.items.trash(get().library, keys)
    set({ selected: get().selected.filter((k) => !keys.includes(k)) })
    await Promise.all([get().refresh(), get().reloadSidebar()])
  },


  /** Copy something about the selection to the clipboard. Returns how many
   *  items contributed, so the caller can report accurately. */
  async copySelected(kind) {
    const s = get()
    const chosen = s.items.filter((i) => s.selected.includes(i.key))
    const lines = chosen
      .map((item) => {
        switch (kind) {
          case 'title':
            return String(item.title ?? '')
          case 'doi':
            return String(item.DOI ?? '')
          case 'url':
            return String(item.url ?? item.DOI ? `https://doi.org/${String(item.DOI)}` : '')
          case 'citation': {
            const authors = item.creators.map((c) => c.lastName || c.name || '').join(', ')
            const y = /\d{4}/.exec(String(item.date ?? ''))?.[0] ?? 'n.d.'
            const venue = String(item.publicationTitle ?? item.bookTitle ?? '')
            return [authors, `(${y})`, String(item.title ?? ''), venue].filter(Boolean).join('. ')
          }
        }
      })
      .filter((l) => l.trim().length > 0)

    if (lines.length) await navigator.clipboard.writeText(lines.join('\n'))
    return lines.length
  },

  async setPluginEnabled(id, enabled) {
    await api.plugins.setEnabled(id, enabled)
    set({ plugins: await api.plugins.list() })
  },

  async reloadPlugins() {
    set({ plugins: await api.plugins.reload() })
  },

  async reindex() {
    const s = get()
    await api.maintenance.reindex(s.library)
    await Promise.all([get().refresh(), get().reloadSidebar()])
  },
}))
