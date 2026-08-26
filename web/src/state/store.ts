/** Application state.
 *
 *  A single store: this is one screen with tightly coupled panes, and splitting
 *  it would only add ceremony. Server data and UI data are kept in separate
 *  slices so it stays obvious which is which.
 */
import { create } from 'zustand'

import { api, connectEvents } from '../api/client'
import { useOverlays } from '../ui/overlays'
import type {
  Collection,
  Item,
  ListQuery,
  PluginStatus,
  Schema,
  SearchMode,
  Stats,
  Tag,
} from '../api/types'

export type View = 'library' | 'trash' | 'collection'
export type Panel = 'detail' | 'plugins' | 'stats'

interface State {
  // connection & metadata
  ready: boolean
  connected: boolean
  error: string | null
  library: number
  schema: Schema | null
  stats: Stats | null
  plugins: PluginStatus[]

  // navigation
  view: View
  collection: string | null
  collections: Collection[]
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
  toggleTag: (tag: string) => void
  clearFilters: () => void
  select: (key: string, additive?: boolean) => void
  moveCursor: (delta: number) => void
  setPanel: (p: Panel) => void
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
  plugins: [],

  view: 'library',
  collection: null,
  collections: [],
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
      const ping = await api.ping()
      const [schema, collections] = await Promise.all([api.schema(), api.collections.list(ping.defaultLibrary)])
      set({ library: ping.defaultLibrary, schema, collections, ready: true, error: null })
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
      const [collections, tags, stats, plugins] = await Promise.all([
        api.collections.list(s.library),
        api.tags.facets(s.library, {
          collection: s.view === 'collection' ? s.collection ?? undefined : undefined,
          trash: s.view === 'trash' ? 'only' : 'exclude',
          limit: 80,
        }),
        api.stats(),
        api.plugins.list(),
      ])
      set({ collections, tags, stats, plugins })
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
      title: '新建条目',
      fields: [
        {
          name: 'title',
          label: '标题',
          required: true,
          autoFocus: true,
          placeholder: '文献标题',
        },
        {
          name: 'itemType',
          label: '类型',
          type: 'select',
          defaultValue: 'journalArticle',
          options: types.map((t) => ({ value: t.type, label: t.label })),
        },
      ],
      confirmLabel: '创建',
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

  async removeCollection(key) {
    const s = get()
    await api.collections.remove(s.library, key)
    if (s.collection === key) get().openLibrary()
    await get().reloadSidebar()
  },

  async addSelectedToCollection(key) {
    const s = get()
    if (!s.selected.length) return
    await api.items.addToCollection(s.library, key, s.selected)
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
