/** Collections, smart collections and tags: what the sidebar shows.
 *
 *  These are the library's *organisation* rather than its contents, and they
 *  change together — creating a collection, filing an item and renaming a tag
 *  all end with the same reload. Keeping them in one place is what stops that
 *  reload being forgotten in one path out of six.
 */
import type { StateCreator } from 'zustand'

import { api } from '../../api/client'
import type { AgentStatus, Collection, SmartCollection, Tag } from '../../api/types'
import type { CollectionValues } from '../../components/CollectionEditor'
import type { State } from '../store'

export interface SidebarSlice {
  collections: Collection[]
  smartCollections: SmartCollection[]
  tags: Tag[]

  reloadSidebar: () => Promise<void>
  createSmart: (name: string, query: string) => Promise<void>
  updateSmart: (key: string, patch: { name?: string; query?: string }) => Promise<void>
  removeSmart: (key: string) => Promise<void>
  toggleTag: (tag: string) => void
  saveCollection: (key: string | null, values: CollectionValues) => Promise<void>
  createCollection: (name: string, parentKey?: string) => Promise<void>
  renameCollection: (key: string, name: string) => Promise<void>
  moveCollection: (key: string, parentKey: string | null) => Promise<void>
  removeCollection: (key: string) => Promise<void>
  addToCollection: (collection: string, keys: string[]) => Promise<void>
  tagItems: (tag: string, keys: string[]) => Promise<void>
  addSelectedToCollection: (key: string) => Promise<void>
  tagSelected: (tag: string) => Promise<void>
}

export const createSidebarSlice: StateCreator<State, [], [], SidebarSlice> = (set, get) => ({
  collections: [],
  smartCollections: [],
  tags: [],

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
      // Colours are remembered by name across the whole session, not just for
      // the tags this view happens to show: the facet list changes with the
      // filter, and a chip must not lose its colour because the sidebar is
      // showing a narrower set.
      const tagColours = { ...get().tagColours }
      for (const tag of tags) {
        if (tag.color) tagColours[tag.name] = tag.color
      }

      set({
        collections,
        smartCollections,
        conversations,
        tags,
        tagColours,
        stats,
        plugins,
        badgeDefs,
        agent,
      })
    } catch {
      /* sidebar is decoration; never block the main view on it */
    }
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

  async addSelectedToCollection(key) {
    await get().addToCollection(key, get().selected)
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
})
