import { useMemo, useState } from 'react'

import { useT } from '../i18n'
import { collectionColour, collectionIcon } from '../lib/collections'
import {
  COLLECTION_COLUMNS,
  gridTemplate,
  totalColumnWidth,
  visibleColumns,
  type ColumnDef,
} from '../lib/columns'
import { rankMatches } from '../lib/fuzzy'
import { compact, shortDate } from '../lib/format'
import { useStore } from '../state/store'
import { Badge, Empty, Icon, contextMenu } from '../ui'
import { tabId } from '../lib/tabs'
import { collectionMenu, smartMenu } from '../components/menus'

interface Entry {
  key: string
  name: string
  smart: boolean
  query: string
  itemCount: number
  created: number
  modified: number
  color?: string
  icon?: string
}

type SortKey = string

/** How each column's cell is styled; anything unlisted is a plain cell. */
const CELL_CLASS: Record<string, string> = {
  name: 'cell name-cell',
  items: 'cell num',
  created: 'cell dim',
  modified: 'cell dim',
  rule: 'cell dim',
}

/**
 * Every collection in one sortable table.
 *
 * The sidebar deliberately shows only the first handful; this is where the rest
 * live, because a list long enough to need scrolling is a list that wants
 * searching and sorting rather than more vertical space.
 */
export function CollectionsPage() {
  const t = useT()
  const collections = useStore((s) => s.collections)
  const smartCollections = useStore((s) => s.smartCollections)
  const openCollection = useStore((s) => s.openCollection)
  const openSmart = useStore((s) => s.openSmart)
  const openCollectionEditor = useStore((s) => s.openCollectionEditor)
  const openTab = useStore((s) => s.openTab)
  const collection = useStore((s) => s.collection)
  const setCollection = (key: string) => useStore.setState({ collection: key })

  const filter = useStore((s) => s.filter)
  const [sort, setSort] = useState<SortKey>('name')
  const [descending, setDescending] = useState(false)

  const entries = useMemo<Entry[]>(
    () => [
      ...collections.map((c) => ({
        key: c.key,
        name: c.name,
        smart: false,
        query: '',
        itemCount: c.itemCount,
        created: c.dateAdded ?? 0,
        modified: c.dateModified ?? 0,
        color: c.color,
        icon: c.icon,
      })),
      ...smartCollections.map((c) => ({
        key: c.key,
        name: c.name,
        smart: true,
        query: c.query,
        itemCount: c.itemCount ?? 0,
        created: c.dateAdded ?? 0,
        modified: c.dateModified ?? 0,
        color: c.color,
        icon: c.icon,
      })),
    ],
    [collections, smartCollections],
  )

  const visible = useMemo(() => {
    const matched = filter ? rankMatches(filter, entries, (e) => `${e.name} ${e.query}`) : entries
    const direction = descending ? -1 : 1
    // Fuzzy matching already ranks by relevance, so sorting is only applied
    // when the user has asked for an order of their own.
    if (filter && sort === 'name') return matched
    return [...matched].sort((a, b) => {
      if (sort === 'items') return direction * (a.itemCount - b.itemCount)
      if (sort === 'kind') return direction * (Number(a.smart) - Number(b.smart))
      // A collection with no recorded date sorts last either way rather than
      // crowding the top as if it were the oldest thing in the library.
      if (sort === 'created' || sort === 'modified') {
        const [x, y] = [a[sort], b[sort]]
        if (!x || !y) return (!x ? 1 : 0) - (!y ? 1 : 0)
        return direction * (x - y)
      }
      return direction * a.name.localeCompare(b.name)
    })
  }, [entries, filter, sort, descending])

  const order = useStore((s) => s.columnOrders.collections)
  const widths = useStore((s) => s.columnWidths)
  const columns = useMemo<ColumnDef[]>(
    () => visibleColumns(order, COLLECTION_COLUMNS),
    [order],
  )
  const template = useMemo(() => `${gridTemplate(columns, widths)} 28px`, [columns, widths])
  // As the item table does: the columns have real widths, so the content can
  // be wider than the pane. Head and rows sit in one scroller at one width, or
  // they drift apart the moment the pane narrows -- which is what opening the
  // detail panel does, and why the rows looked staggered.
  const width = useMemo(() => totalColumnWidth(columns, widths) + 28, [columns, widths])

  /** One cell's contents. Kept beside the catalogue so adding a column is one
   *  entry there and one arm here, rather than a new row layout. */
  const cell = (entry: Entry, id: string) => {
    if (id === 'name') return <span className="name">{entry.name}</span>
    if (id === 'kind')
      return (
        <Badge tone={entry.smart ? 'accent' : 'default'}>
          {t(entry.smart ? 'collections.kind.smart' : 'collections.kind.plain')}
        </Badge>
      )
    if (id === 'items') return compact(entry.itemCount)
    // Collections made before the library recorded dates have none, and an
    // em dash says "not known" where a fabricated date would not.
    if (id === 'created') return entry.created ? shortDate(entry.created) : '—'
    if (id === 'modified') return entry.modified ? shortDate(entry.modified) : '—'
    if (id === 'rule') return entry.query
    return null
  }

  const hint = (entry: Entry, id: string) =>
    id === 'name' ? entry.name : id === 'rule' ? entry.query : undefined

  const header = (key: SortKey, label: string) => (
    <button
      className={sort === key ? 'sorted' : undefined}
      onClick={() => {
        setDescending(sort === key ? !descending : false)
        setSort(key)
      }}
    >
      {label}
      {sort === key && <span className="sort-arrow">{descending ? '↓' : '↑'}</span>}
    </button>
  )

  /** Show a collection in its own tab, so two can be compared side by side. */
  const open = (entry: Entry, keep = false) => {
    openTab({
      id: tabId('library', entry.key),
      kind: 'library',
      title: entry.name,
      target: entry.key,
      preview: !keep,
    })
    if (entry.smart) openSmart(entry.key)
    else openCollection(entry.key)
  }

  return (
    <div className="collections-browser">
      <div className="browser-scroll">
      <div
        className="table-head browser-grid"
        style={{ gridTemplateColumns: template, minWidth: width }}
      >
        {columns.map((c) =>
          c.sort ? (
            <span key={c.id}>{header(c.sort, t(c.labelKey))}</span>
          ) : (
            <button key={c.id} disabled>
              {t(c.labelKey)}
            </button>
          ),
        )}
        <span />
      </div>

      <div className="browser-body" style={{ minWidth: width }}>
        {visible.length === 0 && <Empty>{t('collections.none')}</Empty>}
        {visible.map((entry) => {
          const Glyph = collectionIcon(entry.icon, entry.smart ? 'Smart' : 'Folder')
          const source = entry.smart
            ? smartCollections.find((c) => c.key === entry.key)
            : collections.find((c) => c.key === entry.key)
          return (
            <div
              key={entry.key}
              className="row browser-grid"
              // The same tracks as the header. Set on the header alone, the
              // rows fell back to the stylesheet's five fixed columns and no
              // cell lined up with its heading.
              style={{ gridTemplateColumns: template }}
              data-colour={collectionColour(entry.color)}
              data-selected={collection === entry.key}
              // A click inspects; opening a tab is a deliberate second gesture,
              // so browsing the list does not keep changing what is in front.
              onClick={() => setCollection(entry.key)}
              onDoubleClick={() => open(entry, true)}
              onContextMenu={contextMenu(() =>
                entry.smart && source && 'query' in source
                  ? smartMenu(source)
                  : collectionMenu(source as never),
              )}
            >
              {columns.map((c) => (
                <div key={c.id} className={CELL_CLASS[c.id] ?? 'cell'} title={hint(entry, c.id)}>
                  {c.id === 'name' && <Glyph className="glyph" />}
                  {cell(entry, c.id)}
                </div>
              ))}
              <div className="cell">
                <button
                  className="icon-btn"
                  title={t('menu.edit')}
                  onClick={() => openCollectionEditor(entry.key)}
                >
                  <Icon.Settings size={12} />
                </button>
              </div>
            </div>
          )
        })}
      </div>
      </div>
    </div>
  )
}
