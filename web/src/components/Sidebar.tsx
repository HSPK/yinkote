import { useMemo } from 'react'

import { compact } from '../lib/format'
import { buildTree } from '../lib/tree'
import { useStore } from '../state/store'
import { contextMenu, promptFor, withToast } from '../ui'
import { collectionMenu, libraryMenu, newSmartCollection, smartMenu, trashMenu } from './menus' 
import { useT } from '../i18n'

export function Sidebar() {
  const t = useT()
  const view = useStore((s) => s.view)
  const collection = useStore((s) => s.collection)
  const collections = useStore((s) => s.collections)
  const smartCollections = useStore((s) => s.smartCollections)
  const openSmart = useStore((s) => s.openSmart)
  const tags = useStore((s) => s.tags)
  const activeTags = useStore((s) => s.activeTags)
  const stats = useStore((s) => s.stats)
  const openLibrary = useStore((s) => s.openLibrary)
  const openTrash = useStore((s) => s.openTrash)
  const openCollection = useStore((s) => s.openCollection)
  const toggleTag = useStore((s) => s.toggleTag)
  const createCollection = useStore((s) => s.createCollection)

  const tree = useMemo(() => buildTree(collections), [collections])

  return (
    <nav className="pane">
      <div className="nav-group">
        <button
          className="nav-item"
          data-active={view === 'library'}
          onClick={openLibrary}
          onContextMenu={contextMenu(libraryMenu)}
        >
          <span className="glyph">▤</span>
          <span className="label">{t('sidebar.library')}</span>
          <span className="count">{stats ? compact(stats.items) : ''}</span>
        </button>
        <button
          className="nav-item"
          data-active={view === 'trash'}
          onClick={openTrash}
          onContextMenu={contextMenu(trashMenu)}
        >
          <span className="glyph">⌫</span>
          <span className="label">{t('sidebar.trash')}</span>
          <span className="count">{stats ? compact(stats.trashed) : ''}</span>
        </button>
      </div>

      <div className="nav-group">
        <div className="nav-title">
          {t('sidebar.smart')}
          <button title={t('sidebar.newSmart')} onClick={() => void newSmartCollection()}>
            +
          </button>
        </div>
        {smartCollections.length === 0 && (
          <div className="empty" style={{ padding: '8px 12px' }}>{t('sidebar.empty')}</div>
        )}
        {smartCollections.map((sc) => (
          <button
            key={sc.key}
            className="nav-item"
            data-active={view === 'smart' && collection === sc.key}
            onClick={() => openSmart(sc.key)}
            onContextMenu={contextMenu(() => smartMenu(sc))}
            title={sc.query || sc.name}
          >
            <span className="glyph">⌕</span>
            <span className="label">{sc.name}</span>
            <span className="count">{sc.itemCount ?? ''}</span>
          </button>
        ))}
      </div>

      <div className="nav-group">
        <div className="nav-title">
          {t('sidebar.collections')}
          <button
            title={t('sidebar.newCollection')}
            onClick={async () => {
              const name = await promptFor(t('dialog.newCollection'), {
                label: t('dialog.name'),
              })
              if (name) {
                await withToast(() => createCollection(name), {
                  success: t('toast.created', { name }),
                  failure: t('toast.createFailed'),
                })
              }
            }}
          >
            +
          </button>
        </div>
        {tree.length === 0 && (
          <div className="empty" style={{ padding: '8px 12px' }}>{t('sidebar.empty')}</div>
        )}
        {tree.map((c) => (
          <button
            key={c.key}
            className="nav-item"
            data-active={view === 'collection' && collection === c.key}
            style={{ paddingLeft: 8 + c.depth * 12 }}
            onClick={() => openCollection(c.key)}
            onContextMenu={contextMenu(() => collectionMenu(c))}
            title={c.name}
          >
            <span className="glyph">{c.children.length ? '▾' : '·'}</span>
            <span className="label">{c.name}</span>
            <span className="count">{c.itemCount || ''}</span>
          </button>
        ))}
      </div>

      <div className="nav-group">
        <div className="nav-title">
          {activeTags.length > 0
            ? t('sidebar.tags.selected', { count: activeTags.length })
            : t('sidebar.tags')}
        </div>
        <div className="tag-cloud">
          {tags.map((tag) => (
            <button
              key={tag.name}
              className="tag-chip"
              data-active={activeTags.includes(tag.name)}
              onClick={() => toggleTag(tag.name)}
              title={`${tag.name} · ${tag.count}`}
            >
              {tag.name}
              <span className="n">{tag.count}</span>
            </button>
          ))}
          {tags.length === 0 && (
            <span className="empty" style={{ padding: 0 }}>{t('sidebar.noTags')}</span>
          )}
        </div>
      </div>
    </nav>
  )
}
