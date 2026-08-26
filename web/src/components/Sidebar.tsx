import { useMemo, useState } from 'react'

import { compact } from '../lib/format'
import { accepts, beginDrag, endDrag } from '../lib/dnd'
import { buildTree } from '../lib/tree'
import { useStore } from '../state/store'
import { Icon, contextMenu, confirmAction, promptFor, withToast } from '../ui'
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
  const addToCollection = useStore((s) => s.addToCollection)
  const moveCollection = useStore((s) => s.moveCollection)
  const tagItems = useStore((s) => s.tagItems)
  const trashItems = useStore((s) => s.trashItems)
  const [dropTarget, setDropTarget] = useState<string | null>(null)

  /** Shared drop plumbing: highlight while hovering, clear on the way out. */
  const dropZone = (id: string, willAccept: () => boolean, onDrop: () => Promise<void>) => ({
    'data-drop': dropTarget === id || undefined,
    onDragOver: (e: React.DragEvent) => {
      if (!willAccept()) return
      e.preventDefault()
      e.dataTransfer.dropEffect = 'move'
      if (dropTarget !== id) setDropTarget(id)
    },
    onDragLeave: () => setDropTarget((current) => (current === id ? null : current)),
    onDrop: (e: React.DragEvent) => {
      e.preventDefault()
      setDropTarget(null)
      endDrag()
      void withToast(onDrop, { failure: t('toast.dropFailed') })
    },
  })
  const conversations = useStore((s) => s.conversations)
  const conversation = useStore((s) => s.conversation)
  const openConversation = useStore((s) => s.openConversation)
  const newConversation = useStore((s) => s.newConversation)
  const renameConversation = useStore((s) => s.renameConversation)
  const removeConversation = useStore((s) => s.removeConversation)

  const tree = useMemo(() => buildTree(collections), [collections])

  return (
    <nav className="sidebar-nav">
      <div className="nav-group">
        <button
          className="nav-item"
          data-active={view === 'library'}
          onClick={openLibrary}
          onContextMenu={contextMenu(libraryMenu)}
          {...dropZone(
            'root',
            () => !!accepts('collection'),
            async () => {
              const dragged = accepts('collection')
              if (dragged) await moveCollection(dragged.key, null)
            },
          )}
        >
          <Icon.Library className="glyph" />
          <span className="label">{t('sidebar.library')}</span>
          <span className="count">{stats ? compact(stats.items) : ''}</span>
        </button>
        <button
          className="nav-item"
          data-active={view === 'trash'}
          onClick={openTrash}
          onContextMenu={contextMenu(trashMenu)}
          {...dropZone(
            'trash',
            () => !!accepts('items'),
            async () => {
              const dragged = accepts('items')
              if (dragged) await trashItems(dragged.keys)
            },
          )}
        >
          <Icon.Trash className="glyph" />
          <span className="label">{t('sidebar.trash')}</span>
          <span className="count">{stats ? compact(stats.trashed) : ''}</span>
        </button>
      </div>

      <div className="nav-group">
        <div className="nav-title">
          {t('sidebar.smart')}
          <button title={t('sidebar.newSmart')} onClick={() => void newSmartCollection()}>
            <Icon.Plus size={11} />
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
            <Icon.Smart className="glyph" />
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
            <Icon.Plus size={11} />
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
            draggable
            onDragStart={(e) => beginDrag(e, { kind: 'collection', key: c.key }, c.name)}
            onDragEnd={endDrag}
            {...dropZone(
              `c:${c.key}`,
              // The server rejects cycles, but refusing the drop outright means
              // the user never sees an error for a gesture that was never valid.
              () => !!accepts('items') || accepts('collection')?.key !== c.key,
              async () => {
                const items = accepts('items')
                if (items) return addToCollection(c.key, items.keys)
                const moved = accepts('collection')
                if (moved) await moveCollection(moved.key, c.key)
              },
            )}
          >
            {c.children.length ? (
              <Icon.FolderOpen className="glyph" />
            ) : (
              <Icon.Folder className="glyph" />
            )}
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
              {...dropZone(
                `t:${tag.name}`,
                () => !!accepts('items'),
                async () => {
                  const dragged = accepts('items')
                  if (dragged) await tagItems(tag.name, dragged.keys)
                },
              )}
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

      <div className="nav-group">
        <div className="nav-title">
          {t('sidebar.chat')}
          <button title={t('chat.new')} onClick={() => void newConversation()}>
            <Icon.Plus size={11} />
          </button>
        </div>
        {conversations.length === 0 && (
          <div className="empty" style={{ padding: '8px 12px' }}>{t('chat.empty')}</div>
        )}
        {conversations.map((c) => (
          <button
            key={c.key}
            className="nav-item"
            data-active={view === 'chat' && conversation === c.key}
            onClick={() => void openConversation(c.key)}
            onContextMenu={contextMenu(() => [
              {
                label: t('menu.rename'),
                run: async () => {
                  const title = await promptFor(t('chat.rename'), {
                    label: t('dialog.name'),
                    defaultValue: c.title,
                  })
                  if (title) await renameConversation(c.key, title)
                },
              },
              {
                label: t('menu.delete'),
                danger: true,
                run: async () => {
                  if (await confirmAction(t('chat.confirmDelete', { name: c.title }))) {
                    await removeConversation(c.key)
                  }
                },
              },
            ])}
            title={c.title}
          >
            <Icon.Chat className="glyph" />
            <span className="label">{c.title}</span>
            <span className="count">{c.messageCount || ''}</span>
          </button>
        ))}
      </div>
    </nav>
  )
}
