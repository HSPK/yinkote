/** Thin typed wrapper over the REST API.
 *
 *  One place knows about URLs, error shape and JSON encoding, so components
 *  never touch `fetch` directly.
 */
import type {
  Collection,
  Item,
  ListQuery,
  Page,
  PluginStatus,
  Schema,
  SearchHit,
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

function qs(query: ListQuery): string {
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

export const api = {
  ping: () => request<{ ok: boolean; version: string; defaultLibrary: number }>('/ping'),
  schema: () => request<Schema>('/schema'),
  stats: () => request<Stats>('/stats'),
  libraries: () => request<{ id: number; name: string; version: number }[]>('/libraries'),

  items: {
    list: (lib: number, query: ListQuery = {}) =>
      request<Page<Item>>(`/libraries/${lib}/items${qs(query)}`),
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
    addToCollection: (lib: number, collection: string, keys: string[]) =>
      request<{ added: number }>(`/libraries/${lib}/collections/${collection}/items`, {
        method: 'POST',
        ...json({ keys }),
      }),
  },

  collections: {
    list: (lib: number) => request<Collection[]>(`/libraries/${lib}/collections`),
    create: (lib: number, name: string, parentKey?: string) =>
      request<Collection>(`/libraries/${lib}/collections`, {
        method: 'POST',
        ...json({ name, parentKey }),
      }),
    rename: (lib: number, key: string, name: string) =>
      request<Collection>(`/libraries/${lib}/collections/${key}`, {
        method: 'PATCH',
        ...json({ name }),
      }),
    remove: (lib: number, key: string) =>
      request<{ deleted: number }>(`/libraries/${lib}/collections/${key}`, { method: 'DELETE' }),
  },

  tags: {
    list: (lib: number, q?: string) =>
      request<Tag[]>(`/libraries/${lib}/tags${q ? `?q=${encodeURIComponent(q)}` : ''}`),
    facets: (lib: number, query: ListQuery = {}) =>
      request<Tag[]>(`/libraries/${lib}/facets${qs(query)}`),
    rename: (lib: number, from: string, to: string) =>
      request<{ updated: number }>(`/libraries/${lib}/tags`, { method: 'PATCH', ...json({ from, to }) }),
    remove: (lib: number, name: string) =>
      request<{ deleted: number }>(`/libraries/${lib}/tags`, { method: 'DELETE', ...json({ name }) }),
  },

  search: (lib: number, query: ListQuery) =>
    request<{ hits: SearchHit[]; mode: string; tookMs: number }>(
      `/libraries/${lib}/search${qs(query)}`,
    ),

  plugins: {
    list: () => request<PluginStatus[]>('/plugins'),
    setEnabled: (id: string, enabled: boolean) =>
      request<PluginStatus>(`/plugins/${id}/enabled`, { method: 'POST', ...json({ enabled }) }),
    reload: () => request<PluginStatus[]>('/plugins/reload', { method: 'POST' }),
    call: (id: string, method: string, params: unknown) =>
      request<unknown>(`/plugins/${id}/call`, { method: 'POST', ...json({ method, params }) }),
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
