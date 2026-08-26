/** Thin typed wrapper over the REST API.
 *
 *  One place knows about URLs, error shape and JSON encoding, so components
 *  never touch `fetch` directly.
 */
import type {
  AgentStatus,
  ImportPreview,
  ImportResult,
  BadgeDescriptor,
  BadgeValue,
  CitationRender,
  CitationStyle,
  Collection,
  GraphNeighbourhood,
  Conversation,
  Item,
  Message,
  ListQuery,
  Page,
  PluginStatus,
  QuickAddResponse,
  ResolveResponse,
  ServerInfo,
  Schema,
  SearchHit,
  SmartCollection,
  SourceInfo,
  Stats,
  Tag,
} from './types'

const BASE = '/api/v1'

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(BASE + path, {
    ...init,
    headers: {
      'Content-Type': 'application/json',
      ...(init?.headers ?? {}),
    },
  })
  if (!res.ok) {
    let code = 'error'
    let title = res.statusText
    try {
      const body = await res.json()
      code = body.code ?? code
      title = body.title ?? title
    } catch {
      /* non-JSON error body; the status is enough */
    }
    throw new ApiError(res.status, code, title)
  }
  if (res.status === 204) return undefined as T
  return res.json() as Promise<T>
}

/** Serialise a list/search query. Exported so the encoding is directly
 *  testable — it is the contract with the server's `ListParams`. */
export function buildQuery(query: ListQuery): string {
  const p = new URLSearchParams()
  const add = (k: string, v: unknown) => {
    if (v === undefined || v === null || v === '') return
    p.set(k, String(v))
  }
  add('q', query.q)
  add('mode', query.mode)
  add('collection', query.collection)
  add('trash', query.trash)
  add('sort', query.sort)
  add('direction', query.direction)
  add('limit', query.limit)
  add('offset', query.offset)
  if (query.tag?.length) p.set('tag', query.tag.join(','))
  if (query.itemType?.length) p.set('itemType', query.itemType.join(','))
  const s = p.toString()
  return s ? `?${s}` : ''
}

const json = (body: unknown): RequestInit => ({ body: JSON.stringify(body) })

/** Appearance travels with the name, for either kind of collection.
 *  `null` clears a colour or icon; omitting the field leaves it alone. */
interface CollectionBody {
  name: string
  color?: string | null
  icon?: string | null
}

export const api = {
  ping: () => request<ServerInfo>('/ping'),
  schema: () => request<Schema>('/schema'),
  stats: () => request<Stats>('/stats'),
  libraries: () => request<{ id: number; name: string; version: number }[]>('/libraries'),

  items: {
    list: (lib: number, query: ListQuery = {}) =>
      request<Page<Item>>(`/libraries/${lib}/items${buildQuery(query)}`),
    get: (lib: number, key: string) => request<Item>(`/libraries/${lib}/items/${key}`),
    children: (lib: number, key: string) =>
      request<Item[]>(`/libraries/${lib}/items/${key}/children`),
    create: (lib: number, drafts: unknown[]) =>
      request<{ created: Item[]; failed: { index: number; message: string }[] }>(
        `/libraries/${lib}/items`,
        { method: 'POST', ...json(drafts) },
      ),
    update: (lib: number, key: string, patch: unknown, ifVersion?: number) =>
      request<Item>(`/libraries/${lib}/items/${key}`, {
        method: 'PATCH',
        headers: ifVersion ? { 'If-Unmodified-Since-Version': String(ifVersion) } : {},
        ...json(patch),
      }),
    trash: (lib: number, keys: string[]) =>
      request<{ trashed: number }>(`/libraries/${lib}/items`, {
        method: 'DELETE',
        ...json({ keys }),
      }),
    restore: (lib: number, keys: string[]) =>
      request<{ restored: number }>(`/libraries/${lib}/items/restore`, {
        method: 'POST',
        ...json({ keys }),
      }),
    destroy: (lib: number, keys: string[]) =>
      request<{ deleted: number }>(`/libraries/${lib}/items/delete`, {
        method: 'POST',
        ...json({ keys }),
      }),
    emptyTrash: (lib: number) =>
      request<{ deleted: number }>(`/libraries/${lib}/trash`, { method: 'DELETE' }),
    addToCollection: (lib: number, collection: string, keys: string[]) =>
      request<{ added: number }>(`/libraries/${lib}/collections/${collection}/items`, {
        method: 'POST',
        ...json({ keys }),
      }),
  },

  collections: {
    list: (lib: number) => request<Collection[]>(`/libraries/${lib}/collections`),
    create: (lib: number, body: CollectionBody & { parentKey?: string }) =>
      request<Collection>(`/libraries/${lib}/collections`, { method: 'POST', ...json(body) }),
    update: (lib: number, key: string, body: Partial<CollectionBody>) =>
      request<Collection>(`/libraries/${lib}/collections/${key}`, {
        method: 'PATCH',
        ...json(body),
      }),
    /** `null` moves the collection to the top level. */
    move: (lib: number, key: string, parentKey: string | null) =>
      request<Collection>(`/libraries/${lib}/collections/${key}`, {
        method: 'PATCH',
        ...json({ parentKey }),
      }),
    remove: (lib: number, key: string) =>
      request<{ deleted: number }>(`/libraries/${lib}/collections/${key}`, { method: 'DELETE' }),
  },

  badges: {
    descriptors: () => request<BadgeDescriptor[]>('/badges'),
    resolve: (lib: number, keys: string[]) =>
      request<Record<string, BadgeValue[]>>(`/libraries/${lib}/badges`, {
        method: 'POST',
        ...json({ keys }),
      }),
  },

  graph: {
    around: (lib: number, key: string, limit = 8) =>
      request<GraphNeighbourhood>(`/libraries/${lib}/graph/${key}?limit=${limit}`),
  },

  citations: {
    styles: () => request<CitationStyle[]>('/citation-styles'),
    render: (lib: number, keys: string[], style: string, format: 'text' | 'html' = 'text') =>
      request<CitationRender>(`/libraries/${lib}/citations`, {
        method: 'POST',
        ...json({ keys, style, format }),
      }),
  },

  conversations: {
    list: (lib: number) => request<Conversation[]>(`/libraries/${lib}/conversations`),
    create: (lib: number, body: { title?: string; scope?: string } = {}) =>
      request<Conversation>(`/libraries/${lib}/conversations`, { method: 'POST', ...json(body) }),
    rename: (lib: number, key: string, title: string) =>
      request<Conversation>(`/libraries/${lib}/conversations/${key}`, {
        method: 'PATCH',
        ...json({ title }),
      }),
    remove: (lib: number, key: string) =>
      request<{ deleted: number }>(`/libraries/${lib}/conversations/${key}`, { method: 'DELETE' }),
    messages: (lib: number, key: string) =>
      request<Message[]>(`/libraries/${lib}/conversations/${key}/messages`),
    append: (lib: number, key: string, body: { role: string; content: string }) =>
      request<Message>(`/libraries/${lib}/conversations/${key}/messages`, {
        method: 'POST',
        ...json(body),
      }),
    ask: (lib: number, key: string, content: string) =>
      request<{ message: Message; truncated: boolean }>(
        `/libraries/${lib}/conversations/${key}/ask`,
        { method: 'POST', ...json({ content }) },
      ),
  },

  agent: () => request<AgentStatus>('/agent'),

  import: {
    /** Counts what would arrive. Reads the file; writes nothing. */
    preview: (path: string) =>
      request<ImportPreview>('/import/zotero/preview', { method: 'POST', ...json({ path }) }),
    zotero: (lib: number, path: string) =>
      request<ImportResult>(`/libraries/${lib}/import/zotero`, {
        method: 'POST',
        ...json({ path }),
      }),
  },

  /** Summarise an item into a note child. */
  summarise: (lib: number, key: string, focus?: string) =>
    request<{ note: Item; model: string; truncated: boolean }>(
      `/libraries/${lib}/items/${key}/summarise`,
      { method: 'POST', ...json({ focus }) },
    ),

  files: {
    /** A browser-loadable address, not a fetch: the viewer streams it itself. */
    url: (lib: number, key: string) => `${BASE}/libraries/${lib}/files/${key}`,
    fetch: (lib: number, key: string, url?: string) =>
      request<{ attachment: Item; bytes: number; url: string }>(
        `/libraries/${lib}/items/${key}/fetch`,
        { method: 'POST', ...json({ url }) },
      ),
  },

  smart: {
    list: (lib: number, counts = false) =>
      request<SmartCollection[]>(`/libraries/${lib}/smart-collections${counts ? '?counts=true' : ''}`),
    create: (lib: number, body: CollectionBody & { query: string; mode?: string }) =>
      request<SmartCollection>(`/libraries/${lib}/smart-collections`, {
        method: 'POST',
        ...json(body),
      }),
    update: (
      lib: number,
      key: string,
      body: Partial<CollectionBody & { query: string; mode: string }>,
    ) =>
      request<SmartCollection>(`/libraries/${lib}/smart-collections/${key}`, {
        method: 'PATCH',
        ...json(body),
      }),
    remove: (lib: number, key: string) =>
      request<{ deleted: number }>(`/libraries/${lib}/smart-collections/${key}`, {
        method: 'DELETE',
      }),
  },

  tags: {
    list: (lib: number, q?: string) =>
      request<Tag[]>(`/libraries/${lib}/tags${q ? `?q=${encodeURIComponent(q)}` : ''}`),
    facets: (lib: number, query: ListQuery = {}) =>
      request<Tag[]>(`/libraries/${lib}/facets${buildQuery(query)}`),
    rename: (lib: number, from: string, to: string) =>
      request<{ updated: number }>(`/libraries/${lib}/tags`, { method: 'PATCH', ...json({ from, to }) }),
    remove: (lib: number, name: string) =>
      request<{ deleted: number }>(`/libraries/${lib}/tags`, { method: 'DELETE', ...json({ name }) }),
  },

  search: (lib: number, query: ListQuery) =>
    request<{ hits: SearchHit[]; mode: string; tookMs: number }>(
      `/libraries/${lib}/search${buildQuery(query)}`,
    ),

  /** Identifier detection and metadata lookup. */
  scrape: {
    sources: () => request<SourceInfo[]>('/resolve/sources'),
    resolve: (text: string, limit?: number) =>
      request<ResolveResponse>('/resolve', { method: 'POST', ...json({ text, limit }) }),
    quickAdd: (
      lib: number,
      body: { text: string; collection?: string; tags?: string[]; allowDuplicates?: boolean },
    ) => request<QuickAddResponse>(`/libraries/${lib}/quick-add`, { method: 'POST', ...json(body) }),
  },

  plugins: {
    list: () => request<PluginStatus[]>('/plugins'),
    setEnabled: (id: string, enabled: boolean) =>
      request<PluginStatus>(`/plugins/${id}/enabled`, { method: 'POST', ...json({ enabled }) }),
    reload: () => request<PluginStatus[]>('/plugins/reload', { method: 'POST' }),
    call: (id: string, method: string, params: unknown) =>
      request<unknown>(`/plugins/${id}/call`, { method: 'POST', ...json({ method, params }) }),
  },

  settings: {
    get: () => request<Record<string, unknown>>('/settings'),
    put: (values: Record<string, unknown>) =>
      request<{ ok: boolean }>('/settings', { method: 'PUT', ...json(values) }),
  },

  maintenance: {
    reindex: (lib: number) =>
      request<{ reindexed: number }>(`/maintenance/reindex/${lib}`, { method: 'POST' }),
    optimize: () => request<{ ok: boolean }>('/maintenance/optimize', { method: 'POST' }),
  },
}

/** Live change feed with automatic backoff-and-resume. */
export function connectEvents(onEvent: (e: Record<string, unknown>) => void): () => void {
  let socket: WebSocket | null = null
  let closed = false
  let attempt = 0
  let timer: number | undefined

  const open = () => {
    if (closed) return
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    socket = new WebSocket(`${proto}://${location.host}${BASE}/events`)
    socket.onopen = () => {
      attempt = 0
      onEvent({ type: 'connected' })
    }
    socket.onmessage = (ev) => {
      try {
        onEvent(JSON.parse(ev.data))
      } catch {
        /* ignore malformed frames */
      }
    }
    socket.onclose = () => {
      if (closed) return
      onEvent({ type: 'disconnected' })
      // Exponential backoff, capped so reconnection stays responsive.
      const delay = Math.min(1000 * 2 ** attempt++, 15000)
      timer = window.setTimeout(open, delay)
    }
    socket.onerror = () => socket?.close()
  }

  open()
  return () => {
    closed = true
    if (timer) window.clearTimeout(timer)
    socket?.close()
  }
}
