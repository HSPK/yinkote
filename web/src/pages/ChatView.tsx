import { useEffect, useRef, useState } from 'react'

import { useT } from '../i18n'
import type { Message } from '../api/types'
import { useStore } from '../state/store'
import { Empty } from '../ui'

/** One assistant turn's tool traffic, as stored beside the answer. */
interface ChatStep {
  toolCalls?: { name: string; arguments: unknown }[]
}

/**
 * The transcript for the selected conversation.
 *
 * Turns are persisted server-side, so a thread survives a reload and can later
 * be replayed into the agent loop without the UI owning any of that state.
 */
/** The tool calls behind an answer, folded away until asked for.
 *
 *  Shown at all because an answer built from searches the reader cannot see is
 *  an answer they have no way to check; folded because most of the time they
 *  do not want to. */
function Steps({ steps }: { steps: ChatStep[] }) {
  const t = useT()
  const calls = steps.flatMap((s) => s.toolCalls ?? [])
  if (!calls.length) return null

  return (
    <details className="steps">
      <summary>{t('chat.steps', { count: calls.length })}</summary>
      {calls.map((call, i) => (
        <div key={i} className="step">
          <code>{call.name}</code>
          <span className="dim">{JSON.stringify(call.arguments)}</span>
        </div>
      ))}
    </details>
  )
}

function Turn({ message }: { message: Message }) {
  const t = useT()
  const meta = message.meta as
    | { model?: string; truncated?: boolean; steps?: ChatStep[] }
    | undefined

  return (
    <div className="bubble" data-role={message.role}>
      <div className="bubble-role">
        {t(`chat.role.${message.role}`)}
        {meta?.model && <span className="bubble-model">{meta.model}</span>}
      </div>
      <div className="bubble-body">{message.content}</div>
      {meta?.steps && <Steps steps={meta.steps} />}
      {meta?.truncated && <div className="bubble-note">{t('chat.truncated')}</div>}
    </div>
  )
}

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
          <Turn key={m.id} message={m} />
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
