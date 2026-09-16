import { useMemo } from 'react'

import type { AttachmentKind, BadgeValue, Item, MatchSource } from '../api/types'
import { type MessageKey, useSchemaLabel, useT } from '../i18n'
import {
  allColumns,
  badgeColumn,
  type ColumnDef,
} from '../lib/columns'
import { beginDrag, endDrag } from '../lib/dnd'
import { creatorSummary, displayTitle, modKey, shortDate, snippetParts, year } from '../lib/format'
import { tagColour } from '../lib/tags'
import { searchText } from '../state/scope'
import { useStore } from '../state/store'
import { contextMenu, Icon } from '../ui'
import { itemMenu } from './menus'
import { VirtualList } from './VirtualList'
import { TableHeader, useTableColumns } from './TableHeader'

/**
 * Tags in a table cell.
 *
 * Coloured because that is the whole point of a tag in a dense list: the eye
 * finds a colour in a hundred rows long before it reads a word. The name is
 * still there — a colour alone is a code nobody has been given the key to.
 */
function TagDots({ tags }: { tags: string[] }) {
  const colours = useStore((s) => s.tagColours)
  return (
    <span className="cell-tags">
      {tags.map((tag) => (
        <span key={tag} className="cell-tag" data-colour={tagColour(tag, colours[tag])}>
          {tag}
        </span>
      ))}
    </span>
  )
}

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
  t: ReturnType<typeof useT>
}

/** One glyph per kind of attachment, so a row's contents read at a glance. */
const ATTACH_ICONS: Record<AttachmentKind, (p: { size?: number }) => React.ReactNode> = {
  pdf: Icon.Pdf,
  snapshot: Icon.Globe,
  link: Icon.Globe,
  file: Icon.File,
}

const ATTACH_LABELS: Record<AttachmentKind, MessageKey> = {
  pdf: 'attach.pdf',
  snapshot: 'attach.snapshot',
  link: 'attach.link',
  file: 'attach.file',
}

function AttachmentCell({ item, t }: CellContext) {
  const kinds = item.attachments ?? []
  if (!kinds.length) return null
  return (
    <>
      {kinds.map((k) => {
        const Glyph = ATTACH_ICONS[k]
        return (
          <span key={k} className="attach-mark" data-kind={k} title={t(ATTACH_LABELS[k])}>
            <Glyph size={12} />
          </span>
        )
      })}
    </>
  )
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
      {displayTitle(item, untitled)}
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
  title: { render: TitleCell, title: (i) => displayTitle(i, '') },
  author: { className: 'dim', render: ({ item }) => creatorSummary(item) },
  year: { className: 'num', render: ({ item }) => year(item) },
  type: { className: 'dim', render: ({ typeLabel }) => typeLabel },
  attachments: {
    className: 'attach-cell',
    render: AttachmentCell,
  },
  tags: {
    className: 'dim',
    render: ({ item }) => <TagDots tags={item.tags.map((t) => t.tag)} />,
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
  grid,
}: {
  item: Item
  columns: ColumnDef[]
  selected: boolean
  cursor: boolean
  grid: string
}) {
  const t = useT()
  const label = useSchemaLabel()
  const select = useStore((s) => s.select)
  const selection = useStore((s) => s.selected)
  const openReader = useStore((s) => s.openReader)
  const typeDef = useStore((s) => s.schema?.itemTypes?.find((d) => d.type === item.itemType))
  const badges = useStore((s) => s.badges[item.key])

  const ctx: CellContext = {
    item,
    typeLabel: label(typeDef, item.itemType),
    badges: badges ?? [],
    untitled: t('detail.untitled'),
    t,
  }

  return (
    <div
      className="row"
      style={{ gridTemplateColumns: grid }}
      data-selected={selected}
      data-cursor={cursor}
      onMouseDown={(e) => {
        // Only the primary button changes the selection. This fired for the
        // right button too, so right-clicking inside a selection of twenty
        // collapsed it to the one row under the pointer and the menu that
        // opened a moment later — which knows perfectly well how to act on
        // twenty — was handed one. Every batch action was unreachable by the
        // gesture people actually use to reach it.
        if (e.button !== 0) return
        select(item.key, e.shiftKey ? 'range' : e.metaKey || e.ctrlKey ? 'toggle' : 'none')
      }}
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

/** What "nothing here" means, which is not one thing.
 *
 *  A search that matched nothing, a shelf nobody has filled yet, and a library
 *  that has never had anything in it are three different situations, and only
 *  the last one is somebody's first five minutes with the program. Telling a
 *  new arrival to press a key and type a paper in by hand is the worst of the
 *  three answers: what they almost certainly want is to bring a library they
 *  already have.
 */
function emptyMessage(
  t: ReturnType<typeof useT>,
  s: { query: string; view: string; collection: string | null; total: number },
): string {
  const text = searchText(s)
  if (text) return t('search.empty', { query: text })
  // Scoped to something — a shelf, the trash, a saved search — so the library
  // as a whole is not what is empty.
  if (s.collection || s.view !== 'library') return t('table.emptyHere')
  return t('table.emptyLibrary', { shortcut: modKey() })
}

export function ItemTable() {
  const t = useT()
  const items = useStore((s) => s.items)
  const selected = useStore((s) => s.selected)
  const cursor = useStore((s) => s.cursor)
  const sort = useStore((s) => s.sort)
  const ranked = useStore((s) => s.ranked)
  const direction = useStore((s) => s.direction)
  const setSort = useStore((s) => s.setSort)
  const loading = useStore((s) => s.loading)
  const loadMore = useStore((s) => s.loadMore)
  const query = useStore((s) => s.query)
  const view = useStore((s) => s.view)
  const collection = useStore((s) => s.collection)
  const total = useStore((s) => s.total)

  const badgeDefs = useStore((s) => s.badgeDefs)

  // A position rather than a command: the cursor *is* the identity of the
  // request, so landing on the same row twice asks for nothing new.
  const keepCursorInView = useMemo(() => ({ index: cursor, token: cursor }), [cursor])

  const available = useMemo(() => allColumns(badgeDefs.map((b) => badgeColumn(b))), [badgeDefs])

  /** Labels differ in origin: builtin columns translate, badges carry plugin text. */
  const headerLabel = (c: ColumnDef) =>
    c.badge
      ? (badgeDefs.find((b) => `badge:${b.pluginId}:${b.id}` === c.id)?.label ?? c.badge)
      : t(c.labelKey)

  /** What the header draws. A label always names the column for the tooltip and
   *  for screen readers; a few columns are too narrow to draw the word. */
  const headerContent = (c: ColumnDef) =>
    c.id === 'attachments' ? <Icon.Paperclip size={12} /> : headerLabel(c)

  const layout = useTableColumns('items', { available, label: headerLabel })
  const { columns, grid, width: totalWidth } = layout
  const header = (
    <TableHeader
      layout={layout}
      sort={sort}
      direction={direction}
      onSort={setSort}
      sortDisabled={ranked}
      sortHint={t('table.rankedHint')}
      content={headerContent}
    />
  )

  return (
    <section className="pane main table-pane">
      <VirtualList
        rows={items}
        keyOf={(item) => item.key}
        header={header}
        // Columns have real widths, so the content may be wider than the pane;
        // the header scrolls with it because they share one scroller.
        minWidth={totalWidth}
        scrollTo={keepCursorInView}
        onEndReached={loadMore}
        empty={
          loading ? null : (
            <div className="empty">
              {emptyMessage(t, { query, view, collection, total })}
            </div>
          )
        }
      >
        {(item, index) => (
          <Row
            item={item}
            columns={columns}
            selected={selected.includes(item.key)}
            cursor={index === cursor}
            grid={grid}
          />
        )}
      </VirtualList>
    </section>
  )
}
