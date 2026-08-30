import { useEffect, useState } from 'react'

import { api } from '../api/client'
import type { Message } from '../api/types'
import { useT } from '../i18n'
import { compact, shortDate } from '../lib/format'
import { tabId } from '../lib/tabs'
import { useStore } from '../state/store'
import { Badge, Button, Empty, Icon, contextMenu } from '../ui'
import { conversationMenu } from './menus'

/**
 * What the detail pane shows beside the conversation browser.
 *
 * A conversation is worth more than its title: how long it ran, what it was
 * scoped to, and what was actually asked. The last question is the one that
 * identifies a thread months later — titles drift towards "Untitled" and the
 * first thing you typed does not.
 */
export function ConversationDetail() {
  const t = useT()
  const library = useStore((s) => s.library)
  const conversations = useStore((s) => s.conversations)
  const collections = useStore((s) => s.collections)
  const inspected = useStore((s) => s.inspectedChat)
  const openConversation = useStore((s) => s.openConversation)
  const renameConversation = useStore((s) => s.renameConversation)
  const openTab = useStore((s) => s.openTab)

  const chosen = conversations.find((c) => c.key === inspected)
  const [turns, setTurns] = useState<Message[]>([])

  useEffect(() => {
    if (!chosen) {
      setTurns([])
      return
    }
    let live = true
    void api.conversations
      .messages(library, chosen.key, { limit: 50 })
      .then((page) => {
        if (live) setTurns(page.messages ?? [])
      })
      .catch(() => {
        if (live) setTurns([])
      })
    return () => {
      live = false
    }
  }, [library, chosen?.key, chosen])

  if (!chosen) {
    return (
      <aside className="pane">
        <div className="pane-header">{t('detail.title')}</div>
        <Empty>{t('chats.selectOne')}</Empty>
      </aside>
    )
  }

  const asked = turns.filter((m) => m.role === 'user')
  const scope = collections.find((c) => c.key === chosen.scope)

  const open = () => {
    openTab({ id: tabId('chat', chosen.key), kind: 'chat', title: chosen.title, target: chosen.key })
    void openConversation(chosen.key, true)
  }

  return (
    <aside className="pane" onContextMenu={contextMenu(() => conversationMenu(chosen))}>
      <div className="pane-header">
        {t('detail.title')}
        <span className="spacer" />
        <span style={{ fontFamily: 'var(--mono)' }}>{chosen.key}</span>
      </div>

      <div className="detail">
        {/* Editable in place, because renaming is the one thing anybody does to
            a conversation and a menu is a poor home for it. */}
        <input
          className="detail-title-edit"
          defaultValue={chosen.title}
          key={chosen.key}
          spellCheck={false}
          onBlur={(e) => {
            const next = e.target.value.trim()
            if (next && next !== chosen.title) void renameConversation(chosen.key, next)
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') e.currentTarget.blur()
            if (e.key === 'Escape') {
              e.currentTarget.value = chosen.title
              e.currentTarget.blur()
            }
          }}
        />

        <dl className="field-grid">
          <dt>{t('chats.messages')}</dt>
          <dd>
            <span className="chip-row">{compact(chosen.messageCount)}</span>
          </dd>

          <dt>{t('table.added')}</dt>
          <dd>
            <span className="chip-row">{shortDate(chosen.createdAt) || '—'}</span>
          </dd>

          <dt>{t('table.modified')}</dt>
          <dd>
            <span className="chip-row">{shortDate(chosen.updatedAt) || '—'}</span>
          </dd>

          <dt>{t('chat.scope')}</dt>
          <dd>
            <span className="chip-row">
              {scope ? (
                <Badge tone="accent">{scope.name}</Badge>
              ) : (
                <span className="muted">{t('chat.wholeLibrary')}</span>
              )}
            </span>
          </dd>

          <dt>{t('chats.asked')}</dt>
          <dd>
            <div className="asked-list">
              {asked.length === 0 && <span className="muted">{t('chats.askedNothing')}</span>}
              {asked.slice(0, 6).map((m) => (
                <div key={m.id} className="asked-line" title={m.content}>
                  {m.content}
                </div>
              ))}
            </div>
          </dd>
        </dl>

        <div className="button-row" style={{ padding: '10px 12px' }}>
          <Button onClick={open}>
            <Icon.Chat size={11} /> {t('menu.open')}
          </Button>
        </div>
      </div>
    </aside>
  )
}
