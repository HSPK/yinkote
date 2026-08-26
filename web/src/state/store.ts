/** Application state.
 *
 *  A single store: this is one screen with tightly coupled panes, and splitting
 *  it would only add ceremony. Server data and UI data are kept in separate
 *  slices so it stays obvious which is which.
 */
import { create } from 'zustand'

import { api, connectEvents } from '../api/client'
import { detectLocale, schemaLabel, t, useI18n, type Locale } from '../i18n'
import { DEFAULT_VISIBLE } from '../lib/columns'
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
import { applyTheme, DEFAULT_THEME } from '../lib/theme'
import { useOverlays } from '../ui/overlays'
import type { CollectionValues } from '../components/CollectionEditor'
import type {
  AgentStatus,
  BadgeDescriptor,
  BadgeValue,
  Collection,
  Conversation,
  Message,
  ListQuery,
  PluginStatus,
  Schema,
  ServerInfo,
  SmartCollection,
  Stats,
  Tag,
} from '../api/types'

/** Secondary surfaces. `null` means the workbench itself is in front. */
/** Only preferences remain modal: everything you *work in* is a tab. */
export type Modal = null | 'settings'
export type Panel = 'detail' | 'plugins' | 'stats'

interface State extends Scope {
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
  /** Pane widths in pixels; dragged by the splitters, persisted server-side. */
  layout: { sidebar: number; detail: number }
  /** Item-table column widths in pixels, keyed by column id. */
  columnWidths: Record<string, number>
  /** Visible columns, in display order. */
  columnOrder: string[]
  /** Badge columns offered by plugins. */
  badgeDefs: BadgeDescriptor[]
  /** Resolved badges for the rows currently loaded, keyed by item key. */
  badges: Record<string, BadgeValue[]>
  /** Whether the right-hand detail pane is showing. */
  detailOpen: boolean

  conversations: Conversation[]
  /** Whether a model is configured; the chat box explains itself if not. */
  agent: AgentStatus | null
  /** A question is in flight. */
  asking: boolean
  conversation: string | null
  messages: Message[]
  /** Row height preference, persisted server-side under `ui.`. */
  density: string
  theme: string
  /** Hex accent override, or empty to use the theme's own. */
  accent: string

  // The active tab's library state is projected onto the store, so components
  // read `items` and `query` without knowing which tab they belong to.

  collections: Collection[]
  smartCollections: SmartCollection[]
  tags: Tag[]

  panel: Panel
  paletteOpen: boolean

  bootstrap: () => Promise<void>
  refresh: () => Promise<void>
  loadMore: () => Promise<void>
  listQuery: (offset?: number) => ListQuery
  reloadSidebar: () => Promise<void>
  setQuery: (q: string) => void
  setSort: (field: string) => void
  navigate: (patch: Partial<State>) => void
  openLibrary: () => void
  openTrash: () => void
  openCollection: (key: string) => void
  openSmart: (key: string) => void
  createSmart: (name: string, query: string) => Promise<void>
  updateSmart: (key: string, patch: { name?: string; query?: string }) => Promise<void>
  removeSmart: (key: string) => Promise<void>
  toggleTag: (tag: string) => void
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
  summarise: (itemKey: string) => Promise<void>
  openCollectionEditor: (key: string | null) => void
  saveCollection: (key: string | null, values: CollectionValues) => Promise<void>
  setLayout: (patch: Partial<{ sidebar: number; detail: number }>, commit?: boolean) => void
  setColumnWidth: (id: string, width: number, commit?: boolean) => void
  setColumnOrder: (order: string[]) => void
  resetColumns: () => void
  loadBadges: (keys: string[]) => Promise<void>
  toggleDetail: (open?: boolean) => void
  openConversation: (key: string) => Promise<void>
  newConversation: () => Promise<void>
  renameConversation: (key: string, title: string) => Promise<void>
  removeConversation: (key: string) => Promise<void>
  sendMessage: (text: string) => Promise<void>
  setDensity: (d: string) => void
  setTheme: (id: string, accent?: string) => void
  setLocale: (locale: Locale) => void
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
  createCollection: (name: string, parentKey?: string) => Promise<void>
  renameCollection: (key: string, name: string) => Promise<void>
  moveCollection: (key: string, parentKey: string | null) => Promise<void>
  addToCollection: (collection: string, keys: string[]) => Promise<void>
  tagItems: (tag: string, keys: string[]) => Promise<void>
  trashItems: (keys: string[]) => Promise<void>
  removeCollection: (key: string) => Promise<void>
  addSelectedToCollection: (key: string) => Promise<void>
  tagSelected: (tag: string) => Promise<void>
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

export const useStore = create<State>((set, get) => ({
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
  layout: { sidebar: 232, detail: 380 },
  columnWidths: {},
  columnOrder: DEFAULT_VISIBLE,
  badgeDefs: [],
  badges: {},
  detailOpen: true,
  conversations: [],
  agent: null,
  asking: false,
  conversation: null,
  messages: [],
  density: 'compact',
  theme: DEFAULT_THEME,
  accent: '',

  // The library fields come from one place, so a new one cannot be forgotten
  // here or in `captureScope`.
  ...emptyScope(),

  collections: [],
  smartCollections: [],
  tags: [],

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
      const saved = <T extends string>(key: string): T | undefined =>
        typeof settings[key] === 'string' ? (settings[key] as T) : undefined

      if (saved('ui.density')) get().setDensity(saved('ui.density')!)

      const parsed = <T,>(key: string, fallback: T): T => {
        const raw = saved(key)
        if (!raw) return fallback
        try {
          return JSON.parse(raw) as T
        } catch {
          return fallback
        }
      }
      set({
        layout: parsed('ui.layout', get().layout),
        columnWidths: parsed('ui.columnWidths', {}),
        columnOrder: parsed('ui.columnOrder', DEFAULT_VISIBLE),
        detailOpen: saved('ui.detailOpen') !== 'false',
      })

      const theme = saved('ui.theme') ?? DEFAULT_THEME
      const accent = saved('ui.accent') ?? ''
      set({ theme, accent })
      applyTheme(theme, accent)

      useI18n.getState().setLocale(saved<Locale>('ui.locale') ?? detectLocale())
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

  async reloadSidebar() {
    const s = get()
    try {
      const [collections, smartCollections, conversations, tags, stats, plugins, badgeDefs, agent] =
        await Promise.all([
          api.collections.list(s.library),
          api.smart.list(s.library, true),
          api.conversations.list(s.library),
          api.tags.facets(s.library, {
            collection: s.view === 'collection' ? s.collection ?? undefined : undefined,
            trash: s.view === 'trash' ? 'only' : 'exclude',
            limit: 80,
          }),
          api.stats(),
          api.plugins.list(),
          api.badges.descriptors(),
          api.agent().catch(() => ({ configured: false }) as AgentStatus),
        ])
      set({ collections, smartCollections, conversations, tags, stats, plugins, badgeDefs, agent })
    } catch {
      /* sidebar is decoration; never block the main view on it */
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

  async createSmart(name, query) {
    const created = await api.smart.create(get().library, { name, query })
    await get().reloadSidebar()
    get().openSmart(created.key)
  },

  async updateSmart(key, patch) {
    await api.smart.update(get().library, key, patch)
    await get().reloadSidebar()
    if (get().view === 'smart' && get().collection === key) get().openSmart(key)
  },

  async removeSmart(key) {
    await api.smart.remove(get().library, key)
    if (get().view === 'smart' && get().collection === key) get().openLibrary()
    await get().reloadSidebar()
  },

  toggleTag(tag) {
    const active = get().activeTags
    set({ activeTags: active.includes(tag) ? active.filter((t) => t !== tag) : [...active, tag] })
    void get().refresh()
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

  /** Ask the model for a summary; it lands as a note under the item. */
  async summarise(itemKey) {
    await api.summarise(get().library, itemKey)
    await get().refresh()
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

  /** Create or update either kind of collection.
   *
   *  The two live in different tables because they answer different questions —
   *  one holds items, the other holds a query — but the user made one choice in
   *  one dialog, so the branch belongs here rather than in the UI. */
  async saveCollection(key, values) {
    const s = get()
    const appearance = { name: values.name, color: values.color ?? null, icon: values.icon ?? null }

    if (key) {
      const isSmart = s.smartCollections.some((c) => c.key === key)
      if (isSmart) await api.smart.update(s.library, key, { ...appearance, query: values.query })
      else await api.collections.update(s.library, key, appearance)
      await get().reloadSidebar()
      return
    }

    if (values.smart) {
      const created = await api.smart.create(s.library, { ...appearance, query: values.query })
      await get().reloadSidebar()
      get().openSmart(created.key)
    } else {
      const created = await api.collections.create(s.library, appearance)
      await get().reloadSidebar()
      get().openCollection(created.key)
    }
  },

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

  toggleDetail(open) {
    const detailOpen = open ?? !get().detailOpen
    set({ detailOpen })
    void api.settings.put({ detailOpen: String(detailOpen) })
  },

  async openConversation(key) {
    const title = get().conversations.find((c) => c.key === key)?.title
    set({ conversation: key })
    get().openTab({ id: tabId('chat', key), kind: 'chat', title: title || '', target: key })
    try {
      set({ messages: await api.conversations.messages(get().library, key) })
    } catch {
      set({ messages: [] })
    }
  },

  async newConversation() {
    const created = await api.conversations.create(get().library)
    set({ conversations: [created, ...get().conversations] })
    await get().openConversation(created.key)
  },

  async renameConversation(key, title) {
    await api.conversations.rename(get().library, key, title)
    set({ conversations: await api.conversations.list(get().library) })
  },

  async removeConversation(key) {
    await api.conversations.remove(get().library, key)
    get().closeTab(tabId('chat', key))
    if (get().conversation === key) set({ conversation: null, messages: [] })
    set({ conversations: await api.conversations.list(get().library) })
  },

  /** Ask the agent, or just record the turn when no model is configured.
   *
   *  Either way the question is persisted: a transcript that loses what was
   *  typed because a model was unreachable would be worse than no transcript. */
  async sendMessage(text) {
    const s = get()
    const body = text.trim()
    if (!body || !s.conversation || s.asking) return

    const optimistic: Message = {
      id: -Date.now(),
      role: 'user',
      content: body,
      createdAt: Date.now(),
    }
    set({ messages: [...s.messages, optimistic], asking: true })

    try {
      if (s.agent?.configured) {
        await api.conversations.ask(s.library, s.conversation, body)
      } else {
        await api.conversations.append(s.library, s.conversation, { role: 'user', content: body })
      }
      // Re-read rather than splice: the server may have appended several
      // messages, and it is the one that knows their ids.
      set({ messages: await api.conversations.messages(s.library, s.conversation) })
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) })
      if (s.conversation) {
        set({ messages: await api.conversations.messages(s.library, s.conversation) })
      }
    } finally {
      set({ asking: false })
    }

    // Name an untitled thread from its opening line.
    const current = s.conversations.find((c) => c.key === s.conversation)
    if (current && current.messageCount === 0) {
      const title = body.length > 40 ? `${body.slice(0, 40)}…` : body
      await get().renameConversation(s.conversation, title)
    } else {
      set({ conversations: await api.conversations.list(s.library) })
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

  setLocale(locale) {
    useI18n.getState().setLocale(locale)
    void api.settings.put({ locale })
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

  async createCollection(name, parentKey) {
    const s = get()
    const created = await api.collections.create(s.library, { name, parentKey })
    await get().reloadSidebar()
    get().openCollection(created.key)
  },

  async renameCollection(key, name) {
    await api.collections.update(get().library, key, { name })
    await get().reloadSidebar()
  },

  async moveCollection(key, parentKey) {
    await api.collections.move(get().library, key, parentKey)
    await get().reloadSidebar()
  },

  async removeCollection(key) {
    const s = get()
    await api.collections.remove(s.library, key)
    if (s.collection === key) get().openLibrary()
    await get().reloadSidebar()
  },

  async addSelectedToCollection(key) {
    await get().addToCollection(key, get().selected)
  },

  async addToCollection(collection, keys) {
    if (!keys.length) return
    await api.items.addToCollection(get().library, collection, keys)
    await Promise.all([get().refresh(), get().reloadSidebar()])
  },

  /** Tags the given items, skipping any that already carry the tag so a
   *  repeated drop is a no-op rather than an error. */
  async tagItems(tag, keys) {
    const s = get()
    const name = tag.trim()
    if (!name || !keys.length) return
    for (const key of keys) {
      const item = s.items.find((i) => i.key === key)
      if (!item || item.tags.some((t) => t.tag === name)) continue
      await api.items.update(s.library, key, { tags: [...item.tags, { tag: name, type: 0 }] })
    }
    await Promise.all([get().refresh(), get().reloadSidebar()])
  },

  async trashItems(keys) {
    if (!keys.length) return
    await api.items.trash(get().library, keys)
    set({ selected: get().selected.filter((k) => !keys.includes(k)) })
    await Promise.all([get().refresh(), get().reloadSidebar()])
  },

  async tagSelected(tag) {
    const s = get()
    const name = tag.trim()
    if (!name || !s.selected.length) return
    for (const key of s.selected) {
      const item = s.items.find((i) => i.key === key)
      if (!item || item.tags.some((t) => t.tag === name)) continue
      await api.items.update(s.library, key, { tags: [...item.tags, { tag: name, type: 0 }] })
    }
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
