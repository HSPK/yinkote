import { useVirtualizer } from '@tanstack/react-virtual'
import { useEffect, useMemo, useRef } from 'react'

import type { BadgeValue, Item, MatchSource } from '../api/types'
import { useSchemaLabel, useT } from '../i18n'
import {
  allColumns,
  badgeColumn,
  gridTemplate,
  moveColumn,
  toggleColumn,
  visibleColumns,
  type ColumnDef,
} from '../lib/columns'
import { beginDrag, endDrag } from '../lib/dnd'
import { creatorSummary, shortDate, snippetParts, year } from '../lib/format'
import { useStore } from '../state/store'
import { contextMenu, type MenuItem } from '../ui'
import { itemMenu } from './menus'

const MAX_WIDTH = 640

const SOURCE_GLYPH: Record<MatchSource, string> = {
  keyword: 'K',
  semantic: 'S',
  fuzzy: 'F',
  tag: 'T',
  field: 'D',
}

/** Everything a cell may need, gathered once per row. */
interface CellContext {
  item: Item
  typeLabel: string
  badges: BadgeValue[]
  untitled: string
}

function TitleCell({ item, untitled }: CellContext) {
  const snippet = item.match?.snippet
  return (
    <>
      {item.match?.sources.map((s) => (
        <span key={s} className="src" data-s={s} title={s}>
          {SOURCE_GLYPH[s]}
        </span>
      ))}
      {String(item.title ?? untitled)}
      {snippet && (
        <span className="snippet">
          {snippetParts(snippet).map((p, i) =>
            p.mark ? <mark key={i}>{p.text}</mark> : <span key={i}>{p.text}</span>,
          )}
        </span>
      )}
    </>
  )
}

/** How each builtin column turns an item into cell content.
 *
 *  A lookup table rather than a switch so that adding a column is one entry
 *  here and one in the catalogue, with nothing else to remember. */
const CELLS: Record<
  string,
  {
    className?: string
    render: (ctx: CellContext) => React.ReactNode
    title?: (item: Item) => string
  }
> = {
  title: { render: TitleCell, title: (i) => String(i.title ?? '') },
  author: { className: 'dim', render: ({ item }) => creatorSummary(item) },
  year: { className: 'num', render: ({ item }) => year(item) },
  type: { className: 'dim', render: ({ typeLabel }) => typeLabel },
  tags: {
    className: 'dim',
    render: ({ item }) => item.tags.map((t) => t.tag).join(' · '),
    title: (i) => i.tags.map((t) => t.tag).join(', '),
  },
  publication: {
    className: 'dim',
    render: ({ item }) => String(item.publicationTitle ?? ''),
  },
  modified: { className: 'num', render: ({ item }) => shortDate(item.dateModified) },
  added: { className: 'num', render: ({ item }) => shortDate(item.dateAdded) },
}

function Cell({ column, ctx }: { column: ColumnDef; ctx: CellContext }) {
  if (column.badge) {
    const value = ctx.badges.find((b) => b.badge === column.badge && b.pluginId === column.pluginId)
    return (
      <div className="cell badge-cell">
        {value && (
          <span className="badge-pill" data-tone={value.tone ?? 'neutral'} title={value.title}>
            {value.text}
          </span>
        )}
      </div>
    )
  }

  const spec = CELLS[column.id]
  if (!spec) return <div className="cell" />
  return (
    <div className={spec.className ? `cell ${spec.className}` : 'cell'} title={spec.title?.(ctx.item)}>
      {spec.render(ctx)}
    </div>
  )
}

function Row({
  item,
  columns,
  selected,
  cursor,
  style,
  grid,
}: {
  item: Item
  columns: ColumnDef[]
  selected: boolean
  cursor: boolean
  style: React.CSSProperties
  grid: string
}) {
  const t = useT()
  const label = useSchemaLabel()
  const select = useStore((s) => s.select)
  const selection = useStore((s) => s.selected)
  const openReader = useStore((s) => s.openReader)
  const typeDef = useStore((s) => s.schema?.itemTypes.find((d) => d.type === item.itemType))
  const badges = useStore((s) => s.badges[item.key])

  const ctx: CellContext = {
    item,
    typeLabel: label(typeDef, item.itemType),
    badges: badges ?? [],
    untitled: t('detail.untitled'),
  }

  return (
    <div
      className="row"
      style={{ ...style, gridTemplateColumns: grid }}
      data-selected={selected}
      data-cursor={cursor}
      onMouseDown={(e) =>
        select(item.key, e.shiftKey ? 'range' : e.metaKey || e.ctrlKey ? 'toggle' : 'none')
      }
      onDoubleClick={() => openReader(item.key)}
      onContextMenu={contextMenu(() => itemMenu(item))}
      draggable
      onDragStart={(e) => {
        // Dragging an unselected row acts on that row alone, which is what
        // every file manager does and what the hand expects.
        const keys = selected ? selection : [item.key]
        if (!selected) select(item.key)
        beginDrag(e, { kind: 'items', keys }, `${keys.length} item(s)`)
      }}
      onDragEnd={endDrag}
    >
      {columns.map((c) => (
        <Cell key={c.id} column={c} ctx={ctx} />
      ))}
    </div>
  )
}

export function ItemTable() {
  const t = useT()
  const items = useStore((s) => s.items)
  const selected = useStore((s) => s.selected)
  const cursor = useStore((s) => s.cursor)
  const sort = useStore((s) => s.sort)
  const direction = useStore((s) => s.direction)
  const setSort = useStore((s) => s.setSort)
  const loading = useStore((s) => s.loading)
  const loadMore = useStore((s) => s.loadMore)
  const query = useStore((s) => s.query)

  const badgeDefs = useStore((s) => s.badgeDefs)
  const order = useStore((s) => s.columnOrder)
  const widths = useStore((s) => s.columnWidths)
  const setColumnWidth = useStore((s) => s.setColumnWidth)
  const setColumnOrder = useStore((s) => s.setColumnOrder)

  const available = useMemo(() => allColumns(badgeDefs.map((b) => badgeColumn(b))), [badgeDefs])
  const columns = useMemo(() => visibleColumns(order, available), [order, available])
  const grid = useMemo(() => gridTemplate(columns, widths), [columns, widths])

  /** Labels differ in origin: builtin columns translate, badges carry plugin text. */
  const headerLabel = (c: ColumnDef) =>
    c.badge
      ? (badgeDefs.find((b) => `badge:${b.pluginId}:${b.id}` === c.id)?.label ?? c.badge)
      : t(c.labelKey)

  // Reads the live order at click time: a menu's items are captured when it
  // opens, so anything acting on `order` from the closure would be stale.
  const reorder = (id: string, delta: number) =>
    setColumnOrder(moveColumn(useStore.getState().columnOrder, id, delta))

  const headerMenu = (c: ColumnDef): MenuItem[] => [
    { label: t('table.moveLeft'), onSelect: () => reorder(c.id, -1) },
    { label: t('table.moveRight'), onSelect: () => reorder(c.id, 1) },
    {},
    { label: t('table.hideColumn'), onSelect: () => setColumnOrder(hideColumn(c.id)) },
  ]

  const hideColumn = (id: string) =>
    toggleColumn(useStore.getState().columnOrder, id, available)

  // Resizing tracks the pointer on the window so the drag survives leaving the
  // 5px grip, which is otherwise almost impossible to stay inside.
  const startResize = (c: ColumnDef) => (e: React.PointerEvent) => {
    e.preventDefault()
    e.stopPropagation()
    const cell = (e.currentTarget as HTMLElement).parentElement
    const from = e.clientX
    const base = cell?.getBoundingClientRect().width ?? c.min
    let last = base
    const move = (ev: PointerEvent) => {
      last = Math.max(c.min, Math.min(MAX_WIDTH, base + ev.clientX - from))
      setColumnWidth(c.id, last)
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      document.body.style.cursor = ''
      setColumnWidth(c.id, last, true)
    }
    document.body.style.cursor = 'col-resize'
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  const scrollRef = useRef<HTMLDivElement>(null)
  const rows = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 26,
    overscan: 16,
  })

  // Keep the keyboard cursor inside the viewport.
  useEffect(() => {
    if (items.length) rows.scrollToIndex(cursor, { align: 'auto' })
  }, [cursor, items.length, rows])

  // Fetch the next page as the last rendered rows come into view. Driven by the
  // virtualiser rather than a scroll handler, so it also fires when the cursor
  // is walked to the end with the keyboard.
  const virtual = rows.getVirtualItems()
  const lastVisible = virtual[virtual.length - 1]?.index ?? 0
  useEffect(() => {
    if (items.length && lastVisible >= items.length - 24) void loadMore()
  }, [lastVisible, items.length, loadMore])

  return (
    <section className="pane main table-pane">
      <div className="table-head" style={{ gridTemplateColumns: grid }}>
        {columns.map((c) => (
          <div key={c.id} className="head-cell" onContextMenu={contextMenu(() => headerMenu(c))}>
            <button
              className={sort === c.sort ? 'sorted' : undefined}
              disabled={!c.sort}
              title={headerLabel(c)}
              onClick={() => c.sort && setSort(c.sort)}
            >
              <span className="head-label">{headerLabel(c)}</span>
              {sort === c.sort && (
                <span className="sort-arrow">{direction === 'asc' ? '↑' : '↓'}</span>
              )}
            </button>
            <span
              className="col-grip"
              onPointerDown={startResize(c)}
              onDoubleClick={() => setColumnWidth(c.id, c.width, true)}
            />
          </div>
        ))}
      </div>

      <div className="table-body" ref={scrollRef}>
        {items.length === 0 && !loading && (
          <div className="empty">
            {query ? t('search.empty', { query }) : t('table.empty', { shortcut: '⌘K' })}
          </div>
        )}
        <div style={{ height: rows.getTotalSize(), position: 'relative' }}>
          {virtual.map((v) => {
            const item = items[v.index]
            if (!item) return null
            return (
              <Row
                key={item.key}
                item={item}
                columns={columns}
                selected={selected.includes(item.key)}
                cursor={v.index === cursor}
                style={{ transform: `translateY(${v.start}px)`, height: v.size }}
                grid={grid}
              />
            )
          })}
        </div>
      </div>

    </section>
  )
}
