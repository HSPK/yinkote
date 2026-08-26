import { useEffect, useRef, useState } from 'react'

import { useT } from '../i18n'
import { useStore } from '../state/store'
import { Empty } from '../ui'

/**
 * The transcript for the selected conversation.
 *
 * Turns are persisted server-side, so a thread survives a reload and can later
 * be replayed into the agent loop without the UI owning any of that state.
 */
export function ChatView() {
  const t = useT()
  const conversation = useStore((s) => s.conversation)
  const conversations = useStore((s) => s.conversations)
  const messages = useStore((s) => s.messages)
  const sendMessage = useStore((s) => s.sendMessage)
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState(false)
  const tail = useRef<HTMLDivElement>(null)

  useEffect(() => {
    tail.current?.scrollIntoView({ block: 'end' })
  }, [messages.length, conversation])

  if (!conversation) {
    return (
      <div className="pane main">
        <Empty>{t('chat.none')}</Empty>
      </div>
    )
  }

  const title = conversations.find((c) => c.key === conversation)?.title || t('chat.untitled')

  const submit = async () => {
    const text = draft.trim()
    if (!text || busy) return
    setBusy(true)
    setDraft('')
    try {
      await sendMessage(text)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="pane main chat">
      <div className="chat-head">{title}</div>

      <div className="chat-log">
        {messages.length === 0 && <Empty>{t('chat.start')}</Empty>}
        {messages.map((m) => (
          <div key={m.id} className="bubble" data-role={m.role}>
            <div className="bubble-role">{t(`chat.role.${m.role}`)}</div>
            <div className="bubble-body">{m.content}</div>
          </div>
        ))}
        {busy && <div className="bubble" data-role="assistant"><div className="bubble-body dim">…</div></div>}
        <div ref={tail} />
      </div>

      <div className="chat-input">
        <textarea
          value={draft}
          rows={2}
          placeholder={t('chat.placeholder')}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              void submit()
            }
          }}
        />
        <button className="primary" disabled={!draft.trim() || busy} onClick={() => void submit()}>
          {t('chat.send')}
        </button>
      </div>
    </div>
  )
}
