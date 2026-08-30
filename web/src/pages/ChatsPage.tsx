import { useMemo, useState } from 'react'

import { useT } from '../i18n'
import { rankMatches } from '../lib/fuzzy'
import { compact, shortDate } from '../lib/format'
import { useStore } from '../state/store'
import { Empty, Icon, contextMenu } from '../ui'
import { conversationMenu } from '../components/menus'
import { tabId } from '../lib/tabs'

type SortKey = 'title' | 'messages' | 'created' | 'updated'

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
  const current = useStore((s) => s.conversation)

  const filter = useStore((s) => s.filter)
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
      <div className="table-head chats-grid">
        {header('title', t('chats.name'))}
        {header('messages', t('chats.messages'))}
        {header('created', t('table.added'))}
        {header('updated', t('table.modified'))}
        <span />
      </div>

      <div className="browser-body">
        {visible.length === 0 && <Empty>{t('chats.none')}</Empty>}
        {visible.map((c) => (
          <div
            key={c.key}
            className="row chats-grid"
            data-selected={current === c.key}
            onDoubleClick={() => open(c.key, c.title)}
            onContextMenu={contextMenu(() => [
              { label: t('menu.open'), onSelect: () => open(c.key, c.title) },
              ...conversationMenu(c),
            ])}
          >
            <div className="cell name-cell" title={c.title}>
              <Icon.Chat className="glyph" size={12} />
              <span className="name">{c.title}</span>
            </div>
            <div className="cell num">{compact(c.messageCount)}</div>
            <div className="cell dim">{shortDate(c.createdAt)}</div>
            <div className="cell dim">{shortDate(c.updatedAt)}</div>
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
  )
}
