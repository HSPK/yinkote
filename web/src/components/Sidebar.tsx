import { useMemo, useState } from 'react'

import { compact } from '../lib/format'
import { beginDrag, dropZone, endDrag } from '../lib/dnd'
import type { DragPayload } from '../lib/dnd'
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

  /** A drop target, with the shared highlight and error reporting applied. */
  const zone = (
    id: string,
    accepts: (payload: DragPayload) => boolean,
    onDrop: (payload: DragPayload) => Promise<void>,
  ) =>
    dropZone({
      id,
      active: dropTarget,
      setActive: setDropTarget,
      accepts,
      onDrop: (payload) => withToast(() => onDrop(payload), { failure: t('toast.dropFailed') }),
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
          {...zone(
            'root',
            (p) => p.kind === 'collection',
            async (p) => {
              if (p.kind === 'collection') await moveCollection(p.key, null)
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
          {...zone(
            'trash',
            (p) => p.kind === 'items',
            async (p) => {
              if (p.kind === 'items') await trashItems(p.keys)
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
          {t('sidebar.collections')}
          <button title={t('sidebar.newSmart')} onClick={() => newSmartCollection()}>
            <Icon.Smart size={11} />
          </button>
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
        {tree.length === 0 && smartCollections.length === 0 && (
          <div className="empty" style={{ padding: '8px 12px' }}>{t('sidebar.empty')}</div>
        )}

        {smartCollections.length > 0 && (
          <div className="nav-subtitle">{t('sidebar.smart')}</div>
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
            {...zone(
              `c:${c.key}`,
              // The server rejects cycles, but refusing the drop outright means
              // the user never sees an error for a gesture that was never valid.
              (p) => p.kind === 'items' || p.key !== c.key,
              async (p) => {
                if (p.kind === 'items') return addToCollection(c.key, p.keys)
                await moveCollection(p.key, c.key)
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
              {...zone(
                `t:${tag.name}`,
                (p) => p.kind === 'items',
                async (p) => {
                  if (p.kind === 'items') await tagItems(tag.name, p.keys)
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
                  if (await confirmAction(t('chat.confirmDelete', { name: c.title || t('chat.untitled') }))) {
                    await removeConversation(c.key)
                  }
                },
              },
            ])}
            title={c.title || t('chat.untitled')}
          >
            <Icon.Chat className="glyph" />
            <span className="label">{c.title || t('chat.untitled')}</span>
            <span className="count">{c.messageCount || ''}</span>
          </button>
        ))}
      </div>
    </nav>
  )
}
