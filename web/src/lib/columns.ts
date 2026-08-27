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
  { id: 'publication', labelKey: 'table.publication', sort: null, width: 160, min: 80 },
  { id: 'modified', labelKey: 'table.modified', sort: 'dateModified', width: 108, min: 72 },
  { id: 'added', labelKey: 'table.added', sort: 'dateAdded', width: 108, min: 72 },
]

export const DEFAULT_VISIBLE = ['title', 'author', 'year', 'type', 'tags', 'modified']

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
  const chosen = order.map((id) => byId.get(id)).filter((c): c is ColumnDef => !!c)
  // Never leave the user with an empty table and no way back.
  return chosen.length ? chosen : available.filter((c) => c.id === 'title')
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

/** Toggle a column's visibility, keeping the catalogue's order for new entries. */
export function toggleColumn(order: string[], id: string, available: ColumnDef[]): string[] {
  if (order.includes(id)) {
    const next = order.filter((c) => c !== id)
    return next.length ? next : order
  }
  const rank = new Map(available.map((c, i) => [c.id, i]))
  return [...order, id].sort((a, b) => (rank.get(a) ?? 0) - (rank.get(b) ?? 0))
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
