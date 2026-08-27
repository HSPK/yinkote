import { useEffect, useRef, useState } from 'react'

import { useT } from '../i18n'
import type { Message } from '../api/types'
import { useStore } from '../state/store'
import { Empty, Icon } from '../ui'

/** One entry in an assistant turn, in the order it happened. */
type Step =
  | { kind: 'text'; content: string }
  | { kind: 'tool'; name: string; arguments: unknown; result: string; writes: boolean }

/**
 * A tool call, drawn where it happened.
 *
 * An answer assembled from searches the reader cannot see is one they have no
 * way to check, and a list of calls *after* the answer loses the thing that
 * makes it checkable: which step led to which. So each call sits between the
 * sentence that prompted it and the sentence that followed it.
 *
 * The arguments are always visible — they are short, and they are the part that
 * says what was actually asked. The result is folded, because it is often a
 * page of JSON.
 */
function ToolStep({ step }: { step: Extract<Step, { kind: 'tool' }> }) {
  const t = useT()
  return (
    <div className="step" data-writes={step.writes || undefined}>
      <div className="step-head">
        <Icon.Plugin size={11} />
        <code>{step.name}</code>
        {step.writes && <span className="step-writes">{t('chat.changed')}</span>}
      </div>
      <div className="step-args">{JSON.stringify(step.arguments)}</div>
      {step.result && (
        <details className="step-result">
          <summary>{t('chat.result')}</summary>
          <pre>{step.result}</pre>
        </details>
      )}
    </div>
  )
}

function Turn({ message }: { message: Message }) {
  const t = useT()
  const meta = message.meta as
    | { model?: string; truncated?: boolean; trace?: Step[] }
    | undefined
  const trace = meta?.trace ?? []

  return (
    <div className="bubble" data-role={message.role}>
      <div className="bubble-role">
        {t(`chat.role.${message.role}`)}
        {meta?.model && <span className="bubble-model">{meta.model}</span>}
      </div>

      {/* The trace is the turn as it unfolded; the final answer is the last
          thing in it, so it is rendered once, after. */}
      {trace.map((step, i) =>
        step.kind === 'tool' ? (
          <ToolStep key={i} step={step} />
        ) : (
          <div key={i} className="bubble-body dim">
            {step.content}
          </div>
        ),
      )}

      {message.content && <div className="bubble-body">{message.content}</div>}
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
