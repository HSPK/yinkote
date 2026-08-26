import { useMemo, useState } from 'react'

import { useT } from '../i18n'
import { collectionColour, collectionIcon } from '../lib/collections'
import { rankMatches } from '../lib/fuzzy'
import { compact } from '../lib/format'
import { useStore } from '../state/store'
import { Badge, Empty, Icon, contextMenu } from '../ui'
import { collectionMenu, smartMenu } from '../components/menus'

interface Entry {
  key: string
  name: string
  smart: boolean
  query: string
  itemCount: number
  color?: string
  icon?: string
}

type SortKey = 'name' | 'items' | 'kind'

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
  const setModal = useStore((s) => s.setModal)

  const [filter, setFilter] = useState('')
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
        color: c.color,
        icon: c.icon,
      })),
      ...smartCollections.map((c) => ({
        key: c.key,
        name: c.name,
        smart: true,
        query: c.query,
        itemCount: c.itemCount ?? 0,
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
      return direction * a.name.localeCompare(b.name)
    })
  }, [entries, filter, sort, descending])

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

  const open = (entry: Entry) => {
    if (entry.smart) openSmart(entry.key)
    else openCollection(entry.key)
    setModal(null)
  }

  return (
    <div className="collections-browser">
      <div className="search browser-search">
        <Icon.Search size={12} className="search-icon" />
        <input
          value={filter}
          autoFocus
          spellCheck={false}
          placeholder={t('collections.search')}
          onChange={(e) => setFilter(e.target.value)}
        />
        <span className="search-mode">{visible.length}</span>
      </div>

      <div className="table-head browser-grid">
        {header('name', t('dialog.name'))}
        {header('kind', t('collections.kind'))}
        {header('items', t('collections.items'))}
        <button disabled>{t('collections.rule')}</button>
      </div>

      <div className="browser-body">
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
              data-colour={collectionColour(entry.color)}
              onDoubleClick={() => open(entry)}
              onContextMenu={contextMenu(() =>
                entry.smart && source && 'query' in source
                  ? smartMenu(source)
                  : collectionMenu(source as never),
              )}
            >
              <div className="cell">
                <Glyph className="glyph" />
                <button className="link" onClick={() => open(entry)}>
                  {entry.name}
                </button>
              </div>
              <div className="cell">
                <Badge tone={entry.smart ? 'accent' : 'default'}>
                  {t(entry.smart ? 'collections.kind.smart' : 'collections.kind.plain')}
                </Badge>
              </div>
              <div className="cell num">{compact(entry.itemCount)}</div>
              <div className="cell dim" title={entry.query}>
                {entry.query}
              </div>
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
  )
}
