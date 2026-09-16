import { useMemo, useState } from 'react'

import type { Conversation } from '../api/types'
import { useT } from '../i18n'
import { rankMatches } from '../lib/fuzzy'
import { TableHeader, useTableColumns } from '../components/TableHeader'
import { compact, shortDate } from '../lib/format'
import { useStore } from '../state/store'
import { Empty, Icon, contextMenu } from '../ui'
import { conversationMenu } from '../components/menus'
import { tabId } from '../lib/tabs'

type SortKey = string

/** How each column's cell is styled; anything unlisted is a plain cell. */
const CELL_CLASS: Record<string, string> = {
  title: 'cell name-cell',
  messages: 'cell num',
  created: 'cell dim',
  updated: 'cell dim',
  scope: 'cell dim',
}

/**
 * Every conversation in one sortable table.
 *
 * The sidebar shows the recent handful and now sends the rest here, the way
 * collections already worked: a list that has outgrown a shortcut list wants
 * searching and sorting rather than an expander that makes the sidebar long.
 */
export function ChatsPage() {
  const t = useT()
  const conversations = useStore((s) => s.conversations)
  const openConversation = useStore((s) => s.openConversation)
  const openTab = useStore((s) => s.openTab)
  const inspected = useStore((s) => s.inspectedChat)

  const filter = useStore((s) => s.filter)
  const collections = useStore((s) => s.collections)
  const layout = useTableColumns('chats', { actionsWidth: 28 })
  const { columns, grid: template, width } = layout

  const [sort, setSort] = useState<SortKey>('updated')
  const [descending, setDescending] = useState(true)

  const visible = useMemo(() => {
    const matched = filter
      ? rankMatches(filter, conversations, (c) => c.title)
      : conversations
    const direction = descending ? -1 : 1
    if (filter && sort === 'title') return matched
    return [...matched].sort((a, b) => {
      if (sort === 'messages') return direction * (a.messageCount - b.messageCount)
      if (sort === 'created') return direction * (a.createdAt - b.createdAt)
      if (sort === 'updated') return direction * (a.updatedAt - b.updatedAt)

      return direction * a.title.localeCompare(b.title)
    })
  }, [conversations, filter, sort, descending])

  /** One cell's contents. Adding a column is an entry in the catalogue and an
   *  arm here, rather than a new row layout. */
  const cell = (c: Conversation, id: string) => {
    if (id === 'title') return <span className="name">{c.title || t('chat.untitled')}</span>
    if (id === 'messages') return compact(c.messageCount)
    if (id === 'created') return shortDate(c.createdAt) || '—'
    if (id === 'updated') return shortDate(c.updatedAt) || '—'
    if (id === 'scope')
      return collections.find((x) => x.key === c.scope)?.name ?? t('chat.wholeLibrary')
    return null
  }

  const hint = (c: Conversation, id: string) => (id === 'title' ? c.title : undefined)

  const open = (key: string, title: string) => {
    openTab({ id: tabId('chat', key), kind: 'chat', title, target: key })
    void openConversation(key, true)
  }

  return (
    <div className="collections-browser">
      <div className="browser-scroll">
      <TableHeader
        layout={layout}
        className="chats-grid"
        sort={sort}
        direction={descending ? 'desc' : 'asc'}
        onSort={(key) => {
          setDescending(sort === key ? !descending : key === 'created' || key === 'updated')
          setSort(key)
        }}
      />

      <div className="browser-body" style={{ minWidth: width }}>
        {visible.length === 0 && <Empty>{t('chats.none')}</Empty>}
        {visible.map((c) => (
          <div
            key={c.key}
            className="row chats-grid"
            style={{ gridTemplateColumns: template }}
            data-selected={inspected === c.key}
            // A click inspects, a double-click opens — the same pair the
            // collection browser uses, so browsing a list never costs you the
            // conversation you had in front of you.
            onClick={() => useStore.setState({ inspectedChat: c.key })}
            onDoubleClick={() => open(c.key, c.title)}
            onContextMenu={contextMenu(() => [
              { label: t('menu.open'), onSelect: () => open(c.key, c.title) },
              ...conversationMenu(c),
            ])}
          >
            {columns.map((col) => (
              <div key={col.id} className={CELL_CLASS[col.id] ?? 'cell'} title={hint(c, col.id)}>
                {col.id === 'title' && <Icon.Chat className="glyph" size={12} />}
                {cell(c, col.id)}
              </div>
            ))}
            <div className="cell">
              <button
                className="icon-btn"
                title={t('menu.open')}
                onClick={() => open(c.key, c.title)}
              >
                <Icon.ChevronRight size={12} />
              </button>
            </div>
          </div>
        ))}
      </div>
      </div>
    </div>
  )
}
