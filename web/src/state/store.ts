/** Application state.
 *
 *  A single store: this is one screen with tightly coupled panes, and splitting
 *  it would only add ceremony. Server data and UI data are kept in separate
 *  slices so it stays obvious which is which.
 */
import { create } from 'zustand'

import { api, connectEvents } from '../api/client'
import { detectLocale, schemaLabel, t, useI18n, type Locale } from '../i18n'
import { applyTheme, DEFAULT_THEME } from '../lib/theme'
import { useOverlays } from '../ui/overlays'
import type {
  Collection,
  Conversation,
  Item,
  Message,
  ListQuery,
  PluginStatus,
  Schema,
  SearchMode,
  ServerInfo,
  SmartCollection,
  Stats,
  Tag,
} from '../api/types'

export type View = 'library' | 'trash' | 'collection' | 'smart' | 'chat'

/** Secondary surfaces. `null` means the workbench itself is in front. */
export type Modal = null | 'plugins' | 'status' | 'settings'
export type Panel = 'detail' | 'plugins' | 'stats'

interface State {
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
  /** Pane widths in pixels; dragged by the splitters, persisted server-side. */
  layout: { sidebar: number; detail: number }
  /** Item-table column widths in pixels, keyed by column id. */
  columns: Record<string, number>

  conversations: Conversation[]
  conversation: string | null
  messages: Message[]
  /** Row height preference, persisted server-side under `ui.`. */
  density: string
  theme: string
  /** Hex accent override, or empty to use the theme's own. */
  accent: string

  // navigation
  view: View
  collection: string | null
  collections: Collection[]
  smartCollections: SmartCollection[]
  tags: Tag[]
  activeTags: string[]
  typeFilter: string[]

  // query
  query: string
  mode: SearchMode
  sort: string
  direction: 'asc' | 'desc'

  // results
  items: Item[]
  total: number
  loading: boolean
  tookMs: number

  // selection & UI
  selected: string[]
  cursor: number
  panel: Panel
  paletteOpen: boolean

  bootstrap: () => Promise<void>
  refresh: () => Promise<void>
  reloadSidebar: () => Promise<void>
  setQuery: (q: string) => void
  setMode: (m: SearchMode) => void
  setSort: (field: string) => void
  openLibrary: () => void
  openTrash: () => void
  openCollection: (key: string) => void
  openSmart: (key: string) => void
  createSmart: (name: string, query: string) => Promise<void>
  updateSmart: (key: string, patch: { name?: string; query?: string }) => Promise<void>
  removeSmart: (key: string) => Promise<void>
  toggleTag: (tag: string) => void
  clearFilters: () => void
  select: (key: string, additive?: boolean) => void
  moveCursor: (delta: number) => void
  setPanel: (p: Panel) => void
  setModal: (m: Modal) => void
  setLayout: (patch: Partial<{ sidebar: number; detail: number }>, commit?: boolean) => void
  setColumn: (id: string, width: number, commit?: boolean) => void
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
  layout: { sidebar: 232, detail: 380 },
  columns: {},
  conversations: [],
  conversation: null,
  messages: [],
  density: 'compact',
  theme: DEFAULT_THEME,
  accent: '',

  view: 'library',
  collection: null,
  collections: [],
  smartCollections: [],
  tags: [],
  activeTags: [],
  typeFilter: [],

  query: '',
  mode: 'hybrid',
  sort: 'dateModified',
  direction: 'desc',

  items: [],
  total: 0,
  loading: false,
  tookMs: 0,

  selected: [],
  cursor: 0,
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
      if (saved('ui.searchMode')) set({ mode: saved<SearchMode>('ui.searchMode')! })

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
        columns: parsed('ui.columns', {}),
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
          void api.plugins.list().then((plugins) => set({ plugins }))
        }
      })
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e), ready: true })
    }
  },

  async refresh() {
    const s = get()
    const seq = ++requestSeq
    set({ loading: true })
    const started = performance.now()
    const query: ListQuery = {
      q: s.query || undefined,
      mode: s.query ? s.mode : undefined,
      collection: s.view === 'collection' ? s.collection ?? undefined : undefined,
      tag: s.activeTags.length ? s.activeTags : undefined,
      itemType: s.typeFilter.length ? s.typeFilter : undefined,
      trash: s.view === 'trash' ? 'only' : 'exclude',
      sort: s.sort,
      direction: s.direction,
      limit: PAGE,
    }
    try {
      const page = await api.items.list(s.library, query)
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
    } catch (e) {
      if (seq !== requestSeq) return
      set({ loading: false, error: e instanceof Error ? e.message : String(e) })
    }
  },

  async reloadSidebar() {
    const s = get()
    try {
      const [collections, smartCollections, conversations, tags, stats, plugins] =
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
      ])
      set({ collections, smartCollections, conversations, tags, stats, plugins })
    } catch {
      /* sidebar is decoration; never block the main view on it */
    }
  },

  setQuery(q) {
    set({ query: q })
    if (debounce) window.clearTimeout(debounce)
    debounce = window.setTimeout(() => void get().refresh(), DEBOUNCE_MS)
  },

  setMode(mode) {
    set({ mode })
    void api.settings.put({ searchMode: mode })
    void get().refresh()
  },

  setSort(field) {
    const s = get()
    const direction = s.sort === field && s.direction === 'desc' ? 'asc' : 'desc'
    set({ sort: field, direction })
    void get().refresh()
  },

  openLibrary() {
    set({ view: 'library', collection: null, cursor: 0, selected: [] })
    void get().refresh()
    void get().reloadSidebar()
  },

  openTrash() {
    set({ view: 'trash', collection: null, cursor: 0, selected: [] })
    void get().refresh()
    void get().reloadSidebar()
  },

  openCollection(key) {
    set({ view: 'collection', collection: key, cursor: 0, selected: [] })
    void get().refresh()
    void get().reloadSidebar()
  },

  /** Opening a smart collection *is* running its query: the search box shows
   *  exactly what is being matched, so the result is never mysterious. */
  openSmart(key) {
    const smart = get().smartCollections.find((s) => s.key === key)
    if (!smart) return
    set({
      view: 'smart',
      collection: key,
      query: smart.query,
      mode: smart.mode,
      sort: smart.sort,
      direction: smart.direction,
      activeTags: [],
      cursor: 0,
      selected: [],
    })
    void get().refresh()
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

  select(key, additive = false) {
    const s = get()
    const index = s.items.findIndex((i) => i.key === key)
    if (additive) {
      set({
        selected: s.selected.includes(key)
          ? s.selected.filter((k) => k !== key)
          : [...s.selected, key],
        cursor: index >= 0 ? index : s.cursor,
      })
    } else {
      set({ selected: [key], cursor: index >= 0 ? index : s.cursor, panel: 'detail' })
    }
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

  setLayout(patch, commit) {
    const layout = { ...get().layout, ...patch }
    set({ layout })
    // Only persist when the drag ends; a write per mouse move is pointless.
    if (commit) void api.settings.put({ layout: JSON.stringify(layout) })
  },

  setColumn(id, width, commit) {
    const columns = { ...get().columns, [id]: width }
    set({ columns })
    if (commit) void api.settings.put({ columns: JSON.stringify(columns) })
  },

  async openConversation(key) {
    set({ view: 'chat', conversation: key, selected: [] })
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
    if (get().conversation === key) {
      set({ conversation: null, messages: [] })
      get().openLibrary()
    }
    set({ conversations: await api.conversations.list(get().library) })
  },

  /** Records the user's turn. The assistant reply arrives with the agent loop;
   *  until then the transcript is still real and still persisted. */
  async sendMessage(text) {
    const s = get()
    const body = text.trim()
    if (!body || !s.conversation) return

    const sent = await api.conversations.append(s.library, s.conversation, {
      role: 'user',
      content: body,
    })
    set({ messages: [...get().messages, sent] })

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
    const created = await api.collections.create(s.library, name, parentKey)
    await get().reloadSidebar()
    get().openCollection(created.key)
  },

  async renameCollection(key, name) {
    await api.collections.rename(get().library, key, name)
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
