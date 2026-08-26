import { useMemo } from 'react'

import { compact } from '../lib/format'
import { buildTree } from '../lib/tree'
import { useStore } from '../state/store'
import { contextMenu, promptFor, withToast } from '../ui'
import { collectionMenu, libraryMenu, trashMenu } from './menus' 

export function Sidebar() {
  const view = useStore((s) => s.view)
  const collection = useStore((s) => s.collection)
  const collections = useStore((s) => s.collections)
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
          <span className="label">我的文库</span>
          <span className="count">{stats ? compact(stats.items) : ''}</span>
        </button>
        <button
          className="nav-item"
          data-active={view === 'trash'}
          onClick={openTrash}
          onContextMenu={contextMenu(trashMenu)}
        >
          <span className="glyph">⌫</span>
          <span className="label">回收站</span>
          <span className="count">{stats ? compact(stats.trashed) : ''}</span>
        </button>
      </div>

      <div className="nav-group">
        <div className="nav-title">
          收藏夹
          <button
            title="新建收藏夹"
            onClick={async () => {
              const name = await promptFor('新建收藏夹', {
                label: '名称',
                placeholder: '例如：扩散模型',
              })
              if (name) {
                await withToast(() => createCollection(name), {
                  success: `已创建「${name}」`,
                  failure: '创建收藏夹失败',
                })
              }
            }}
          >
            +
          </button>
        </div>
        {tree.length === 0 && <div className="empty" style={{ padding: '8px 12px' }}>暂无</div>}
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
        <div className="nav-title">标签{activeTags.length > 0 && ` · ${activeTags.length} 已选`}</div>
        <div className="tag-cloud">
          {tags.map((t) => (
            <button
              key={t.name}
              className="tag-chip"
              data-active={activeTags.includes(t.name)}
              onClick={() => toggleTag(t.name)}
              title={`${t.name} · ${t.count} 条`}
            >
              {t.name}
              <span className="n">{t.count}</span>
            </button>
          ))}
          {tags.length === 0 && <span className="empty" style={{ padding: 0 }}>暂无标签</span>}
        </div>
      </div>
    </nav>
  )
}
