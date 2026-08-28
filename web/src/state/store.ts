/** Application state.
 *
 *  A single store: this is one screen with tightly coupled panes, and splitting
 *  it would only add ceremony. Server data and UI data are kept in separate
 *  slices so it stays obvious which is which.
 */
import { create } from 'zustand'

import { ApiError, api, connectEvents, setApiKey } from '../api/client'
import { follow } from '../lib/tasks'
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
  CitationStyle,
  Item,
  ListQuery,
  PluginStatus,
  Schema,
  ServerInfo,
  Stats,
} from '../api/types'

export type Panel = 'detail' | 'plugins' | 'stats'

export interface State extends Scope, PrefsSlice, SidebarSlice, ChatSlice {
  // connection & metadata
  ready: boolean
  connected: boolean
  error: string | null
  /** The server wants an API key and this browser has not got a working one. */
  needsKey: boolean
  useApiKey: (key: string) => Promise<void>
  library: number
  schema: Schema | null
  stats: Stats | null
  server: ServerInfo | null
  plugins: PluginStatus[]

  /** Which secondary surface is open, if any. */
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
  /** Re-read the server's own report of itself: access, connector, versions. */
  refreshServer: () => Promise<void>
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
  /** Preferences are a surface you work in, so they are a tab like the rest. */
  openSettings: () => void
  setFilter: (value: string) => void
  openTab: (tab: Tab) => void
  closeTab: (id: string) => void
  closeTabs: (scope: 'all' | 'others', keep?: string) => void
  activateTab: (id: string) => void
  keepTab: (id: string) => void
  openReader: (itemKey: string, keep?: boolean) => void
  /** Show the relationship graph around an item. */
  openGraph: (itemKey: string, keep?: boolean) => void
  /** Show an item in the detail pane, whether or not the current list holds it.
   *
   *  `select` is the *table's* selection model — an index into the visible
   *  list, so that shift-ranges mean something. A graph neighbour is
   *  deliberately not in that list, and asking `select` for one silently did
   *  nothing at all. */
  showItem: (key: string) => Promise<void>
  /** An item being shown that the current list does not contain. */
  detached: Item | null
  /** What the graph tab is currently showing, for the status bar. */
  graphSize: { nodes: number; edges: number }
  setGraphSize: (nodes: number, edges: number) => void
  /** How many cited-but-unowned works the gaps tab found, for the status bar. */
  gapCount: number
  setGapCount: (count: number) => void
  /** Downloads waiting or failed, for the sidebar badge. */
  downloadCount: number
  setDownloadCount: (count: number) => void
  fileCount: number
  setFileCount: (count: number) => void
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
  /** Copy the selection. A citation is rendered in `style`, or the remembered one. */
  copySelected: (kind: 'title' | 'doi' | 'url' | 'citation', style?: string) => Promise<number>
  /** The styles the server can render, fetched once at startup. */
  citationStyles: CitationStyle[]
  setPluginEnabled: (id: string, enabled: boolean) => Promise<void>
  reloadPlugins: () => Promise<void>
  reindex: () => Promise<void>
  /** Give a tag a colour, or clear it back to the one derived from its name. */
  setTagColour: (name: string, colour: string) => Promise<void>
  /** Stored colours by tag name, for the places that only carry names. */
  tagColours: Record<string, string>
  renameTag: (from: string, to: string) => Promise<void>
  deleteTag: (name: string) => Promise<void>
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
  citationStyles: [],
  error: null,
  library: 1,
  schema: null,
  stats: null,
  server: null,
  plugins: [],
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
  needsKey: false,

  async refreshServer() {
    // One source of truth. The caller often already holds the new status, but
    // trusting a reply over the server's own account is how two views of the
    // same fact start to disagree.
    set({ server: await api.ping() })
  },

  async bootstrap() {
    try {
      const server = await api.ping()
      const [schema, collections, settings, citationStyles] = await Promise.all([
        api.schema(),
        api.collections.list(server.defaultLibrary),
        api.settings.get().catch(() => ({}) as Record<string, unknown>),
        // Menus are built synchronously, so the styles must already be here.
        api.citations.styles().catch(() => []),
      ])
      set({
        library: server.defaultLibrary,
        server,
        schema,
        collections,
        citationStyles,
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
        } else if (type === 'agentProgress') {
          // A turn belongs to the conversation, so progress arrives whether or
          // not the chat tab is in front — and is kept, so switching to it
          // shows the turn already under way rather than an empty pane.
          get().applyRun(String(event.conversation ?? ''), event.state)
        } else if (type === 'pluginsChanged') {
          // Badge columns come and go with their plugin, and any answers the
          // old one gave are no longer trustworthy.
          set({ badges: {} })
          void get().reloadSidebar()
        }
      })
    } catch (e) {
      // A server started with YK_API_KEY answers 401 to everything, and the
      // page has no way to know that except by being told. Without this the
      // workbench sat on "connecting" for ever, which reads as a broken
      // server rather than as a library asking who you are.
      if (e instanceof ApiError && e.status === 401) {
        set({ needsKey: true, ready: true, error: null })
        return
      }
      set({ error: e instanceof Error ? e.message : String(e), ready: true })
    }
  },

  /** Try the key the user just typed by starting again from the top. */
  async useApiKey(key: string) {
    setApiKey(key.trim() || null)
    set({ needsKey: false, ready: false, error: null })
    await get().bootstrap()
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
        approximate: page.approximate ?? false,
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
      set({
        items: [...get().items, ...page.items],
        total: page.total,
        approximate: page.approximate ?? false,
        loadingMore: false,
      })
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
    const s = get()
    const active = s.tabs.find((t) => t.id === s.activeTab)

    // A library view has to land in a tab that can hold one. Writing it into
    // whatever happened to be focused is how clicking a collection from a
    // chat used to do nothing visible while quietly changing a scope nobody
    // could see. Library is not special here — it is opened like any other
    // tab, and it can be closed like any other tab.
    if (!active || !TAB_OWNS_LIBRARY.has(active.kind)) {
      const existing = s.tabs.find((t) => TAB_OWNS_LIBRARY.has(t.kind) && !t.target)
      get().openTab(existing ?? libraryTab(''))
    }

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

  openSettings() {
    get().openTab({ id: tabId('settings'), kind: 'settings', title: '' })
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
    const active = get().activeTab
    const next = tabs.some((t) => t.id === active) ? active : (tabs[0]?.id ?? '')
    if (next !== active) {
      set({ tabs })
      get().activateTab(next)
    } else {
      set({ tabs })
    }
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



  tagColours: {},
  graphSize: { nodes: 0, edges: 0 },
  gapCount: 0,
  downloadCount: 0,
  fileCount: 0,

  setDownloadCount(downloadCount) {
    set({ downloadCount })
  },

  setFileCount(fileCount) {
    set({ fileCount })
  },
  detached: null,

  setGapCount(gapCount) {
    set({ gapCount })
  },

  async showItem(key) {
    const s = get()
    if (s.items.some((i) => i.key === key)) {
      s.select(key)
      return
    }

    // Show the selection immediately and fill in the detail when it arrives;
    // waiting for a round trip to highlight what was clicked feels broken.
    set({ selected: [key], anchor: -1, cursor: -1, panel: 'detail', detached: null })
    const item = await api.items.get(s.library, key).catch(() => null)
    // Another click may have landed first; the newer one owns the pane.
    if (item && get().selected[0] === key) set({ detached: item })
  },

  setGraphSize(nodes, edges) {
    set({ graphSize: { nodes, edges } })
  },

  /** Show what an item sits next to. A preview tab, like the reader: a graph is
   *  usually a glance on the way somewhere else. */
  openGraph(itemKey, keep = false) {
    const title = get().items.find((i) => i.key === itemKey)?.title
    get().openTab({
      id: tabId('graph', itemKey),
      kind: 'graph',
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

  /** Edit a collection in its own tab.
   *
   *  `null` means a new one. Both go through one surface because which kind a
   *  new collection becomes is decided inside it, not by which button opened
   *  it. */
  openCollectionEditor(key) {
    const target = key ?? 'new'
    set({ collectionEditor: key })
    get().openTab({
      id: tabId('collection-edit', target),
      kind: 'collection-edit',
      title: '',
      target,
    })
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
  async copySelected(kind, style) {
    const s = get()
    const chosen = s.items.filter((i) => s.selected.includes(i.key))

    // A reference is rendered by the server, which owns the styles. The client
    // used to assemble one itself, in one shape, badly — and it disagreed with
    // every real style, which is the failure mode nobody proofreads for.
    if (kind === 'citation') {
      if (!chosen.length) return 0
      const chosenStyle = style ?? s.citationStyle
      const rendered = await api.citations.render(
        s.library,
        chosen.map((i) => i.key),
        chosenStyle,
      )
      if (style && style !== s.citationStyle) get().setCitationStyle(style)
      await navigator.clipboard.writeText(rendered.bibliography.join('\n'))
      return rendered.bibliography.length
    }

    const lines = chosen
      .map((item) => {
        switch (kind) {
          case 'title':
            return String(item.title ?? '')
          case 'doi':
            return String(item.DOI ?? '')
          case 'url':
            return String(item.url ?? item.DOI ? `https://doi.org/${String(item.DOI)}` : '')
        }
      })
      .filter((l) => l && l.trim().length > 0) as string[]

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

  async setTagColour(name, colour) {
    await api.tags.setColor(get().library, name, colour)
    await get().reloadSidebar()
  },

  async renameTag(from, to) {
    await api.tags.rename(get().library, from, to)
    await Promise.all([get().refresh(), get().reloadSidebar()])
  },

  async deleteTag(name) {
    await api.tags.remove(get().library, name)
    await Promise.all([get().refresh(), get().reloadSidebar()])
  },

  async reindex() {
    const s = get()
    const { task } = await api.maintenance.reindex(s.library)
    const done = await follow(task.id)
    if (done && done.phase !== 'done') throw new Error(done.error ?? 'reindex failed')
    await Promise.all([get().refresh(), get().reloadSidebar()])
  },
}))
