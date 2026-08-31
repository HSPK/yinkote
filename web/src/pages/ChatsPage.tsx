import { useMemo, useState } from 'react'

import type { Conversation } from '../api/types'
import { useT } from '../i18n'
import { rankMatches } from '../lib/fuzzy'
import {
  CHAT_COLUMNS,
  gridTemplate,
  totalColumnWidth,
  visibleColumns,
  type ColumnDef,
} from '../lib/columns'
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
  const order = useStore((s) => s.columnOrders.chats)
  const widths = useStore((s) => s.columnWidths)
  const columns = useMemo<ColumnDef[]>(() => visibleColumns(order, CHAT_COLUMNS), [order])
  const template = useMemo(() => `${gridTemplate(columns, widths)} 28px`, [columns, widths])
  const width = useMemo(() => totalColumnWidth(columns, widths) + 28, [columns, widths])

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

  const header = (key: SortKey, label: string) => (
    <button
      className={sort === key ? 'sorted' : undefined}
      onClick={() => {
        // Dates read newest-first by default; names read A to Z. Flipping to a
        // date column and landing on the oldest thread is not what was asked.
        setDescending(sort === key ? !descending : key === 'created' || key === 'updated')
        setSort(key)
      }}
    >
      {label}
      {sort === key && <span className="sort-arrow">{descending ? '↓' : '↑'}</span>}
    </button>
  )

  const open = (key: string, title: string) => {
    openTab({ id: tabId('chat', key), kind: 'chat', title, target: key })
    void openConversation(key, true)
  }

  return (
    <div className="collections-browser">
      <div className="browser-scroll">
      <div
        className="table-head chats-grid"
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
