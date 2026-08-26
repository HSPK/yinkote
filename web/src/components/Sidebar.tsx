import { useMemo } from 'react'

import type { Collection } from '../api/types'
import { compact } from '../lib/format'
import { useStore } from '../state/store'

interface Node extends Collection {
  children: Node[]
  depth: number
}

/** Flatten the collection list into render order, preserving hierarchy. */
function toTree(collections: Collection[]): Node[] {
  const nodes = new Map<string, Node>()
  for (const c of collections) nodes.set(c.key, { ...c, children: [], depth: 0 })

  const roots: Node[] = []
  for (const node of nodes.values()) {
    const parent = node.parentKey ? nodes.get(node.parentKey) : undefined
    if (parent) parent.children.push(node)
    else roots.push(node)
  }

  const out: Node[] = []
  const walk = (list: Node[], depth: number) => {
    list.sort((a, b) => a.sortIndex - b.sortIndex || a.name.localeCompare(b.name))
    for (const n of list) {
      n.depth = depth
      out.push(n)
      walk(n.children, depth + 1)
    }
  }
  walk(roots, 0)
  return out
}

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

  const tree = useMemo(() => toTree(collections), [collections])

  return (
    <nav className="pane">
      <div className="nav-group">
        <button
          className="nav-item"
          data-active={view === 'library'}
          onClick={openLibrary}
        >
          <span className="glyph">▤</span>
          <span className="label">我的文库</span>
          <span className="count">{stats ? compact(stats.items) : ''}</span>
        </button>
        <button className="nav-item" data-active={view === 'trash'} onClick={openTrash}>
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
            onClick={() => {
              const name = window.prompt('收藏夹名称')
              if (name?.trim()) void createCollection(name.trim())
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
