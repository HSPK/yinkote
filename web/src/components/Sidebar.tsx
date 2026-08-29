import { useMemo, useState } from 'react'
import { useCapped } from '../lib/useCapped'

import { compact } from '../lib/format'
import { beginDrag, dropZone, endDrag } from '../lib/dnd'
import { collectionColour, collectionIcon } from '../lib/collections'
import { tabId } from '../lib/tabs'
import type { DragPayload } from '../lib/dnd'
import { buildTree } from '../lib/tree'
import { tagColour } from '../lib/tags'
import { useStore } from '../state/store'
import { Icon, contextMenu, confirmAction, promptFor, withToast } from '../ui'
import { newCollection } from './actions'
import { collectionMenu, libraryMenu, smartMenu, tagMenu, trashMenu } from './menus' 
import { useT } from '../i18n'

/** How many rows each sidebar group shows before offering the rest elsewhere. */
const SIDEBAR_LIMIT = 12
/** Tags are chips, so a generous limit turns the sidebar into a wall of them.
 *  Ten is enough to show what a library is mostly about; the rest are one
 *  click away and the search box finds any of them by name. */
const TAG_LIMIT = 10

/** Conversations accumulate faster than anything else in the sidebar — one per
 *  question asked — and unbounded they pushed every other group off screen. */
const CHAT_LIMIT = 10

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
  const openTab = useStore((s) => s.openTab)
  const conversations = useStore((s) => s.conversations)
  const activeTab = useStore((s) => s.activeTab)
  const downloadCount = useStore((s) => s.downloadCount)
  const openConversation = useStore((s) => s.openConversation)
  const newConversation = useStore((s) => s.newConversation)
  const renameConversation = useStore((s) => s.renameConversation)
  const removeConversation = useStore((s) => s.removeConversation)

  const tree = useMemo(() => buildTree(collections), [collections])

  // The sidebar is a shortcut list, not an inventory. Past a point another row
  // stops helping and starts hiding the rows below it, so the rest lives in a
  // browser that can search and sort.
  const shownSmart = smartCollections.slice(0, SIDEBAR_LIMIT)
  const shownTree = tree.slice(0, SIDEBAR_LIMIT)
  const shownTags = useCapped(tags, TAG_LIMIT)
  const shownChats = useCapped(conversations, CHAT_LIMIT)

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

        <button
          className="nav-item"
          data-active={activeTab.startsWith('files')}
          title={t('files.title')}
          onClick={() => openTab({ id: tabId('files'), kind: 'files', title: '' })}
        >
          <Icon.Library className="glyph" />
          <span className="label">{t('files.title')}</span>
        </button>

        <button
          className="nav-item"
          data-active={activeTab.startsWith('downloads')}
          title={t('downloads.title')}
          onClick={() => openTab({ id: tabId('downloads'), kind: 'downloads', title: '' })}
        >
          <Icon.Download className="glyph" />
          <span className="label">{t('downloads.title')}</span>
          {/* Only when something needs attention: a permanent zero is noise. */}
          <span className="count">{downloadCount || ''}</span>
        </button>

        <button
          className="nav-item"
          data-active={activeTab.startsWith('gaps')}
          title={t('gaps.open')}
          onClick={() => openTab({ id: tabId('gaps'), kind: 'gaps', title: '' })}
        >
          <Icon.Graph className="glyph" />
          <span className="label">{t('gaps.title')}</span>
        </button>

        <button
          className="nav-item"
          data-active={activeTab.startsWith('tasks')}
          title={t('tasks.open')}
          onClick={() => openTab({ id: tabId('tasks'), kind: 'tasks', title: '' })}
        >
          <Icon.Gauge className="glyph" />
          <span className="label">{t('tasks.title')}</span>
        </button>

        <button
          className="nav-item"
          data-active={activeTab.startsWith('duplicates')}
          title={t('duplicates.open')}
          onClick={() => openTab({ id: tabId('duplicates'), kind: 'duplicates', title: '' })}
        >
          <Icon.Library className="glyph" />
          <span className="label">{t('duplicates.title')}</span>
        </button>
      </div>

      <div className="nav-group">
        <div className="nav-title">
          {t('sidebar.collections')}
          <button title={t('collection.new')} onClick={() => void newCollection().run()}>
            <Icon.Plus size={11} />
          </button>
        </div>
        {tree.length === 0 && smartCollections.length === 0 && (
          <div className="empty" style={{ padding: '8px 12px' }}>{t('sidebar.empty')}</div>
        )}

        {shownSmart.map((sc) => {
          const Glyph = collectionIcon(sc.icon, 'Smart')
          return (
            <button
              key={sc.key}
              className="nav-item"
              data-active={view === 'smart' && collection === sc.key}
              data-colour={collectionColour(sc.color)}
              onClick={() => openSmart(sc.key)}
              onContextMenu={contextMenu(() => smartMenu(sc))}
              title={sc.query || sc.name}
            >
              <Glyph className="glyph" />
              <span className="label">{sc.name}</span>
              {/* A saved search with words has to be *run* to be counted, and
                  a ranked search stops at a bounded pool — so this number can
                  be a floor. A bare "300" beside a search matching twenty
                  thousand is a wrong answer, not a rounded one. */}
              <span className="count" title={sc.itemCountApproximate ? t('smart.atLeast') : undefined}>
                {sc.itemCount === undefined ? '' : `${sc.itemCount}${sc.itemCountApproximate ? '+' : ''}`}
              </span>
            </button>
          )
        })}

        {shownTree.map((c) => (
          <button
            key={c.key}
            className="nav-item"
            data-active={view === 'collection' && collection === c.key}
            style={{ paddingLeft: 8 + c.depth * 12 }}
            onClick={() => openCollection(c.key)}
            onContextMenu={contextMenu(() => collectionMenu(c))}
            data-colour={collectionColour(c.color)}
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
            {(() => {
              const Glyph = collectionIcon(c.icon, c.children.length ? 'FolderOpen' : 'Folder')
              return <Glyph className="glyph" />
            })()}
            <span className="label">{c.name}</span>
            <span className="count">{c.itemCount || ''}</span>
          </button>
        ))}

        <button
          className="nav-more"
          onClick={() => openTab({ id: tabId('collections'), kind: 'collections', title: '' })}
        >
          {t('sidebar.browseAll')}
        </button>
      </div>

      <div className="nav-group">
        <div className="nav-title">
          {activeTags.length > 0
            ? t('sidebar.tags.selected', { count: activeTags.length })
            : t('sidebar.tags')}
        </div>
        <div className="tag-cloud">
          {shownTags.shown.map((tag) => (
            <button
              key={tag.name}
              className="tag-chip"
              data-active={activeTags.includes(tag.name)}
              data-colour={tagColour(tag.name, tag.color)}
              onClick={() => toggleTag(tag.name)}
              onContextMenu={contextMenu(() => tagMenu(tag))}
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
          {shownTags.hidden > 0 && (
            <button className="tag-chip more" onClick={shownTags.expand}>
              {t('sidebar.more', { count: shownTags.hidden })}
            </button>
          )}
          {shownTags.expanded && shownTags.overflows && (
            <button className="tag-chip more" onClick={shownTags.collapse}>
              {t('sidebar.less')}
            </button>
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
        {shownChats.shown.map((c) => (
          <button
            key={c.key}
            className="nav-item"
            data-active={activeTab === tabId('chat', c.key)}
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
        {shownChats.hidden > 0 && (
          <button className="nav-more" onClick={shownChats.expand}>
            {t('sidebar.more', { count: shownChats.hidden })}
          </button>
        )}
        {shownChats.expanded && shownChats.overflows && (
          <button className="nav-more" onClick={shownChats.collapse}>
            {t('sidebar.less')}
          </button>
        )}
      </div>
    </nav>
  )
}
