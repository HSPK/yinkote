/** The table's column catalogue.
 *
 *  Kept out of the component so that "which columns exist", "which are shown"
 *  and "how wide are they" are one description that the header, the rows and
 *  the column picker all read. Plugins extend the same list with badge columns,
 *  which is why a column is data rather than JSX.
 */
import type { MessageKey } from '../i18n'

export interface ColumnDef {
  id: string
  labelKey: MessageKey
  /** Server-side sort field, or `null` when the column cannot be sorted. */
  sort: string | null
  /** Default width in pixels; `0` means "share whatever is left". */
  width: number
  min: number
  /** Badge columns are contributed by plugins and resolved per item. */
  badge?: string
  /** Where the plugin-contributed badge came from, for the picker's grouping. */
  pluginId?: string
}

export const BUILTIN_COLUMNS: ColumnDef[] = [
  { id: 'title', labelKey: 'table.title', sort: 'title', width: 0, min: 160 },
  { id: 'author', labelKey: 'table.author', sort: 'creator', width: 150, min: 80 },
  { id: 'year', labelKey: 'table.year', sort: 'year', width: 52, min: 44 },
  { id: 'type', labelKey: 'table.type', sort: 'itemType', width: 108, min: 64 },
  { id: 'tags', labelKey: 'table.tags', sort: null, width: 132, min: 64 },
  // Narrow on purpose: it carries glyphs, and the header is a paperclip.
  // Sortable, because "which of these did I actually get the PDF for" is a
  // question people ask of a list — one ordering answers both that and "which
  // have the good kind", since the rank runs pdf, page, link, nothing.
  { id: 'attachments', labelKey: 'table.attachments', sort: 'attachment', width: 58, min: 44 },
  { id: 'publication', labelKey: 'table.publication', sort: null, width: 160, min: 80 },
  { id: 'modified', labelKey: 'table.modified', sort: 'dateModified', width: 108, min: 72 },
  { id: 'added', labelKey: 'table.added', sort: 'dateAdded', width: 108, min: 72 },
]

export const DEFAULT_VISIBLE = ['title', 'author', 'year', 'type', 'tags', 'attachments', 'modified']

/**
 * Which table a set of columns belongs to.
 *
 * The collection browser wanted the same show/hide/reorder behaviour as the
 * item table, and the choice was to copy the machinery or to say which table is
 * being described. Copying would have meant two pickers, two persisted
 * settings and two chances to fix a bug once.
 */
export type TableId = 'items' | 'collections' | 'chats' | 'files' | 'downloads' | 'tasks' | 'gaps'

export const COLLECTION_COLUMNS: ColumnDef[] = [
  { id: 'name', labelKey: 'dialog.name', sort: 'name', width: 0, min: 160 },
  { id: 'kind', labelKey: 'collections.kind', sort: 'kind', width: 96, min: 64 },
  { id: 'items', labelKey: 'collections.items', sort: 'items', width: 72, min: 52 },
  { id: 'created', labelKey: 'table.added', sort: 'created', width: 116, min: 72 },
  { id: 'modified', labelKey: 'table.modified', sort: 'modified', width: 116, min: 72 },
  { id: 'rule', labelKey: 'collections.rule', sort: null, width: 0, min: 100 },
]

export const COLLECTION_DEFAULT_VISIBLE = ['name', 'kind', 'items', 'created', 'rule']

export const CHAT_COLUMNS: ColumnDef[] = [
  { id: 'title', labelKey: 'chats.name', sort: 'title', width: 0, min: 160 },
  { id: 'messages', labelKey: 'chats.messages', sort: 'messages', width: 64, min: 48 },
  { id: 'scope', labelKey: 'chat.scope', sort: null, width: 120, min: 72 },
  { id: 'created', labelKey: 'table.added', sort: 'created', width: 116, min: 72 },
  { id: 'updated', labelKey: 'table.modified', sort: 'updated', width: 116, min: 72 },
]

export const CHAT_DEFAULT_VISIBLE = ['title', 'messages', 'created', 'updated']

export const FILE_COLUMNS: ColumnDef[] = [
  { id: 'name', labelKey: 'files.col.name', sort: null, width: 0, min: 210 },
  { id: 'paper', labelKey: 'files.col.paper', sort: null, width: 0, min: 180 },
  { id: 'source', labelKey: 'files.col.source', sort: null, width: 0, min: 240 },
  { id: 'size', labelKey: 'files.col.size', sort: null, width: 90, min: 64 },
]

export const DOWNLOAD_COLUMNS: ColumnDef[] = [
  { id: 'title', labelKey: 'downloads.col.title', sort: null, width: 0, min: 180 },
  { id: 'url', labelKey: 'downloads.col.url', sort: null, width: 0, min: 200 },
  { id: 'state', labelKey: 'downloads.col.state', sort: null, width: 260, min: 120 },
  { id: 'size', labelKey: 'downloads.col.size', sort: null, width: 80, min: 64 },
]

export const TASK_COLUMNS: ColumnDef[] = [
  { id: 'job', labelKey: 'tasks.col.job', sort: null, width: 120, min: 80 },
  { id: 'state', labelKey: 'tasks.col.state', sort: null, width: 0, min: 220 },
  { id: 'outcome', labelKey: 'tasks.col.outcome', sort: null, width: 0, min: 220 },
  { id: 'started', labelKey: 'tasks.col.started', sort: null, width: 84, min: 72 },
]

export const GAP_COLUMNS: ColumnDef[] = [
  { id: 'work', labelKey: 'gaps.work', sort: null, width: 0, min: 394 },
  { id: 'year', labelKey: 'gaps.year', sort: null, width: 64, min: 44 },
  { id: 'citedBy', labelKey: 'gaps.citedBy', sort: null, width: 90, min: 64 },
]

/** What each table shows before anybody changes it. */
export const DEFAULT_COLUMNS: Record<TableId, string[]> = {
  items: DEFAULT_VISIBLE,
  collections: COLLECTION_DEFAULT_VISIBLE,
  chats: CHAT_DEFAULT_VISIBLE,
  files: FILE_COLUMNS.map((c) => c.id),
  downloads: DOWNLOAD_COLUMNS.map((c) => c.id),
  tasks: TASK_COLUMNS.map((c) => c.id),
  gaps: GAP_COLUMNS.map((c) => c.id),
}

export const CATALOGUE: Record<TableId, ColumnDef[]> = {
  items: BUILTIN_COLUMNS,
  collections: COLLECTION_COLUMNS,
  chats: CHAT_COLUMNS,
  files: FILE_COLUMNS,
  downloads: DOWNLOAD_COLUMNS,
  tasks: TASK_COLUMNS,
  gaps: GAP_COLUMNS,
}


/** A plugin's badge contribution, turned into a column. */
export function badgeColumn(badge: {
  id: string
  label: string
  pluginId: string
  width?: number
  sortable?: boolean
}): ColumnDef {
  return {
    id: `badge:${badge.pluginId}:${badge.id}`,
    // Badge labels are plugin-authored, so they carry their own text; the
    // catalogue key is unused and the table reads `label` instead.
    labelKey: 'table.badge',
    // The sort key is the column id: the server reads it back apart to know
    // which plugin to ask, so the two cannot drift.
    sort: badge.sortable ? `badge:${badge.pluginId}:${badge.id}` : null,
    width: badge.width ?? 72,
    min: 44,
    badge: badge.id,
    pluginId: badge.pluginId,
  }
}

/** Every column that could be shown, builtin first, then plugin badges. */
export function allColumns(badges: ColumnDef[] = []): ColumnDef[] {
  return [...BUILTIN_COLUMNS, ...badges]
}

/**
 * The ordered, visible columns.
 *
 * Unknown ids are dropped rather than rendered blank, which is what happens
 * when a plugin supplying a badge column is disabled — the layout should
 * survive that, not break.
 */
export function visibleColumns(order: string[], available: ColumnDef[]): ColumnDef[] {
  const byId = new Map(available.map((c) => [c.id, c]))
  const chosen = [...new Set(order)].map((id) => byId.get(id)).filter((c): c is ColumnDef => !!c)
  // Never leave the user with an empty table and no way back.
  return chosen.length ? chosen : available.slice(0, 1)
}

/** CSS grid template for a set of columns, honouring user-set widths. */
export function gridTemplate(columns: ColumnDef[], widths: Record<string, number>): string {
  return columns
    .map((c) => {
      const w = widths[c.id] ?? c.width
      return w > 0 ? `${w}px` : `minmax(${c.min}px, 1fr)`
    })
    .join(' ')
}

/** How wide the columns want to be, in total.
 *
 *  The list needs this to know when the content is wider than the pane and a
 *  sideways scrollbar is called for. Flexible columns contribute their minimum,
 *  which is the width below which they would rather scroll than shrink.
 */
export function totalColumnWidth(
  columns: ColumnDef[],
  widths: Record<string, number>,
): number {
  return columns.reduce((sum, c) => {
    const w = widths[c.id] ?? c.width
    return sum + (w > 0 ? w : c.min)
  }, 0)
}

/** Insert near the catalogue neighbour without re-sorting the user's columns. */
export function toggleColumn(order: string[], id: string, available: ColumnDef[]): string[] {
  if (!available.some((c) => c.id === id)) return order
  if (order.includes(id)) {
    const next = order.filter((c) => c !== id)
    return next.some((key) => available.some((c) => c.id === key)) ? next : order
  }
  const rank = new Map(available.map((c, i) => [c.id, i]))
  const next = [...order]
  const index = order.findIndex((key) => (rank.get(key) ?? -1) > rank.get(id)!)
  next.splice(index < 0 ? next.length : index, 0, id)
  return next
}

/** A drop moves to the target's position, leaving all other columns in order. */
export function reorderColumn(order: string[], id: string, target: string): string[] {
  const from = order.indexOf(id)
  const to = order.indexOf(target)
  return from < 0 || to < 0 ? order : moveColumn(order, id, to - from)
}

/** Item width keys stay compatible; other tables no longer share their widths. */
export function columnWidthKey(table: TableId, id: string): string {
  return table === 'items' ? id : `${table}:${id}`
}

export function restoredColumnOrders(value: unknown, legacy?: unknown): Record<TableId, string[]> {
  const saved = value && typeof value === 'object' ? value as Record<string, unknown> : {}
  return Object.fromEntries(
    (Object.keys(DEFAULT_COLUMNS) as TableId[]).map((table) => {
      const order = saved[table] ?? (table === 'items' ? legacy : undefined)
      const valid = Array.isArray(order) && order.every((id) => typeof id === 'string') && order.length
      return [table, valid ? [...new Set(order)] : [...DEFAULT_COLUMNS[table]]]
    }),
  ) as Record<TableId, string[]>
}

/** Move a column one place left or right, for keyboard and menu reordering. */
export function moveColumn(order: string[], id: string, delta: number): string[] {
  const from = order.indexOf(id)
  if (from < 0) return order
  const to = Math.max(0, Math.min(order.length - 1, from + delta))
  if (to === from) return order
  const next = [...order]
  next.splice(to, 0, ...next.splice(from, 1))
  return next
}
