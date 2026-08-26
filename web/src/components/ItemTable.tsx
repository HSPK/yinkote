import { useVirtualizer } from '@tanstack/react-virtual'
import { useEffect, useMemo, useRef } from 'react'

import type { Item, MatchSource } from '../api/types'
import { creatorSummary, shortDate, snippetParts, year } from '../lib/format'
import { useStore } from '../state/store'
import { beginDrag, endDrag } from '../lib/dnd'
import { contextMenu } from '../ui'
import { itemMenu } from './menus'
import { useSchemaLabel, useT } from '../i18n'

/** Column definitions. `id` keys the persisted widths, so renaming a label or
 *  switching language never disturbs a user's layout. */
const COLUMNS = [
  { id: 'title', field: 'title', key: 'table.title', width: 0, min: 160 },
  { id: 'author', field: 'creator', key: 'table.author', width: 150, min: 80 },
  { id: 'year', field: 'year', key: 'table.year', width: 48, min: 40 },
  { id: 'type', field: 'itemType', key: 'table.type', width: 108, min: 64 },
  { id: 'tags', field: '', key: 'table.tags', width: 132, min: 64 },
  { id: 'modified', field: 'dateModified', key: 'table.modified', width: 108, min: 72 },
] as const

const MAX_WIDTH = 640

/** A width of 0 means "take what is left", which keeps the table filling the
 *  pane at any window size until the user pins that column explicitly. */
function template(widths: Record<string, number>): string {
  return COLUMNS.map((c) => {
    const w = widths[c.id] ?? c.width
    return w > 0 ? `${w}px` : `minmax(${c.min}px, 1fr)`
  }).join(' ')
}

const SOURCE_GLYPH: Record<MatchSource, string> = {
  keyword: 'K',
  semantic: 'S',
  fuzzy: 'F',
  tag: 'T',
  field: 'D',
}

function Row({ item, selected, cursor, style, grid }: {
  item: Item
  selected: boolean
  cursor: boolean
  style: React.CSSProperties
  grid: string
}) {
  const t = useT()
  const select = useStore((s) => s.select)
  const selection = useStore((s) => s.selected)
  const label = useSchemaLabel()
  const typeDef = useStore((s) => s.schema?.itemTypes.find((d) => d.type === item.itemType))
  const snippet = item.match?.snippet

  return (
    <div
      className="row"
      style={{ ...style, gridTemplateColumns: grid }}
      data-selected={selected}
      data-cursor={cursor}
      onMouseDown={(e) => select(item.key, e.metaKey || e.ctrlKey)}
      onContextMenu={contextMenu(() => itemMenu(item))}
      draggable
      onDragStart={(e) => {
        // Dragging an unselected row acts on that row alone, which is what
        // every file manager does and what the hand expects.
        const keys = selected ? selection : [item.key]
        if (!selected) select(item.key, false)
        beginDrag(e, { kind: 'items', keys }, `${keys.length} item(s)`)
      }}
      onDragEnd={endDrag}
    >
      <div className="cell" title={String(item.title ?? '')}>
        {item.match?.sources.map((s) => (
          <span key={s} className="src" data-s={s} title={s}>
            {SOURCE_GLYPH[s]}
          </span>
        ))}
        {String(item.title ?? t('detail.untitled'))}
        {snippet && (
          <span className="snippet">
            {snippetParts(snippet).map((p, i) => (p.mark ? <mark key={i}>{p.text}</mark> : <span key={i}>{p.text}</span>))}
          </span>
        )}
      </div>
      <div className="cell dim">{creatorSummary(item)}</div>
      <div className="cell num">{year(item)}</div>
      <div className="cell dim">{label(typeDef, item.itemType)}</div>
      <div className="cell dim" title={item.tags.map((t) => t.tag).join(', ')}>
        {item.tags.map((t) => t.tag).join(' · ')}
      </div>
      <div className="cell num">{shortDate(item.dateModified)}</div>
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
  const total = useStore((s) => s.total)
  const loading = useStore((s) => s.loading)
  const query = useStore((s) => s.query)

  const columns = useStore((s) => s.columns)
  const setColumn = useStore((s) => s.setColumn)
  const grid = useMemo(() => template(columns), [columns])

  // Resizing tracks the pointer on the window so the drag survives leaving the
  // 4px grip, which is otherwise almost impossible to stay inside.
  const startResize = (id: string, min: number) => (e: React.PointerEvent) => {
    e.preventDefault()
    e.stopPropagation()
    const cell = (e.currentTarget as HTMLElement).parentElement
    const from = e.clientX
    const base = cell?.getBoundingClientRect().width ?? min
    let last = base
    const move = (ev: PointerEvent) => {
      last = Math.max(min, Math.min(MAX_WIDTH, base + ev.clientX - from))
      setColumn(id, last)
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      document.body.style.cursor = ''
      setColumn(id, last, true)
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

  return (
    <section className="pane main table-pane">
      <div className="table-head" style={{ gridTemplateColumns: grid }}>
        {COLUMNS.map((c) => (
          <div key={c.id} className="head-cell">
            <button
              className={sort === c.field ? 'sorted' : undefined}
              disabled={!c.field}
              onClick={() => c.field && setSort(c.field)}
            >
              {t(c.key)}
              {sort === c.field ? (direction === 'asc' ? ' ↑' : ' ↓') : ''}
            </button>
            <span
              className="col-grip"
              onPointerDown={startResize(c.id, c.min)}
              onDoubleClick={() => setColumn(c.id, c.width, true)}
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
          {rows.getVirtualItems().map((v) => {
            const item = items[v.index]
            if (!item) return null
            return (
              <Row
                key={item.key}
                item={item}
                selected={selected.includes(item.key)}
                cursor={v.index === cursor}
                style={{ transform: `translateY(${v.start}px)`, height: v.size }}
                grid={grid}
              />
            )
          })}
        </div>
      </div>

      <div className="pane-header" style={{ borderTop: '1px solid var(--line)', borderBottom: 0 }}>
        {t('table.count', { shown: items.length, total })}
        <span className="spacer" />
        {loading ? t('table.loading') : ''}
      </div>
    </section>
  )
}
