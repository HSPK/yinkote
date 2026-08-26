import { useVirtualizer } from '@tanstack/react-virtual'
import { useEffect, useRef } from 'react'

import type { Item, MatchSource } from '../api/types'
import { creatorSummary, shortDate, snippetParts, year } from '../lib/format'
import { useStore } from '../state/store'
import { contextMenu } from '../ui'
import { itemMenu } from './menus'

/** Column layout, shared by the header and every row so they stay aligned. */
const COLUMNS = '1fr 150px 48px 108px 132px 108px'

const SORTABLE: { field: string; label: string }[] = [
  { field: 'title', label: '标题' },
  { field: 'creator', label: '作者' },
  { field: 'year', label: '年' },
  { field: 'itemType', label: '类型' },
  { field: '', label: '标签' },
  { field: 'dateModified', label: '修改' },
]

const SOURCE_GLYPH: Record<MatchSource, string> = {
  keyword: 'K',
  semantic: 'S',
  fuzzy: 'F',
  tag: 'T',
  field: 'D',
}

function Row({ item, selected, cursor, style }: {
  item: Item
  selected: boolean
  cursor: boolean
  style: React.CSSProperties
}) {
  const select = useStore((s) => s.select)
  const typeLabel = useStore((s) => s.schema?.itemTypes.find((t) => t.type === item.itemType)?.label)
  const snippet = item.match?.snippet

  return (
    <div
      className="row"
      style={{ ...style, gridTemplateColumns: COLUMNS }}
      data-selected={selected}
      data-cursor={cursor}
      onMouseDown={(e) => select(item.key, e.metaKey || e.ctrlKey)}
      onContextMenu={contextMenu(() => itemMenu(item))}
    >
      <div className="cell" title={String(item.title ?? '')}>
        {item.match?.sources.map((s) => (
          <span key={s} className="src" data-s={s} title={s}>
            {SOURCE_GLYPH[s]}
          </span>
        ))}
        {String(item.title ?? '(无标题)')}
        {snippet && (
          <span className="snippet">
            {snippetParts(snippet).map((p, i) => (p.mark ? <mark key={i}>{p.text}</mark> : <span key={i}>{p.text}</span>))}
          </span>
        )}
      </div>
      <div className="cell dim">{creatorSummary(item)}</div>
      <div className="cell num">{year(item)}</div>
      <div className="cell dim">{typeLabel ?? item.itemType}</div>
      <div className="cell dim" title={item.tags.map((t) => t.tag).join(', ')}>
        {item.tags.map((t) => t.tag).join(' · ')}
      </div>
      <div className="cell num">{shortDate(item.dateModified)}</div>
    </div>
  )
}

export function ItemTable() {
  const items = useStore((s) => s.items)
  const selected = useStore((s) => s.selected)
  const cursor = useStore((s) => s.cursor)
  const sort = useStore((s) => s.sort)
  const direction = useStore((s) => s.direction)
  const setSort = useStore((s) => s.setSort)
  const total = useStore((s) => s.total)
  const loading = useStore((s) => s.loading)
  const query = useStore((s) => s.query)

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
    <section className="pane table-pane">
      <div className="table-head" style={{ gridTemplateColumns: COLUMNS }}>
        {SORTABLE.map((c) => (
          <button
            key={c.label}
            className={sort === c.field ? 'sorted' : undefined}
            disabled={!c.field}
            onClick={() => c.field && setSort(c.field)}
          >
            {c.label}
            {sort === c.field ? (direction === 'asc' ? ' ↑' : ' ↓') : ''}
          </button>
        ))}
      </div>

      <div className="table-body" ref={scrollRef}>
        {items.length === 0 && !loading && (
          <div className="empty">
            {query ? `没有匹配 “${query}” 的条目` : '这里还没有条目 — 按 ⌘K 新建'}
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
              />
            )
          })}
        </div>
      </div>

      <div className="pane-header" style={{ borderTop: '1px solid var(--line)', borderBottom: 0 }}>
        {items.length} / {total} 条
        <span className="spacer" />
        {loading ? '载入中…' : ''}
      </div>
    </section>
  )
}
