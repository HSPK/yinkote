import { useEffect, useRef, useState } from 'react'

import { useT } from '../i18n'
import type { Message, RunState, RunStep } from '../api/types'
import { useStore } from '../state/store'
import { Empty, Icon } from '../ui'

/**
 * One step of a turn.
 *
 * The shape is deliberate. A step is a *row* — a fixed-height header with the
 * tool's name on the left and a disclosure caret on the right — and its body
 * grows underneath when opened. The header does not move: everything above it
 * stays where it was, so opening a step to read a result never shifts the
 * sentence you were in the middle of. That is why this is a header plus a body
 * rather than a `<details>` whose summary is part of the flow.
 *
 * Steps are inset to the same left edge as the reply, so a turn reads as one
 * column: thought, action, thought, answer — rather than as prose with
 * machinery bolted to the side.
 */
function Step({ step }: { step: RunStep }) {
  const t = useT()
  const [open, setOpen] = useState(false)

  if (step.kind === 'text') return <div className="turn-text">{step.content}</div>

  const thinking = step.kind === 'thinking'
  const label = thinking ? t('chat.thinking') : step.name
  const body = thinking ? step.content : step.result

  return (
    <div className="turn-step" data-writes={!thinking && step.writes ? '' : undefined}>
      <button className="step-bar" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        <Icon.ChevronDown size={11} className="step-caret" data-open={open || undefined} />
        {thinking ? <Icon.Bulb size={11} /> : <Icon.Plugin size={11} />}
        <span className="step-name">{label}</span>
        {/* What was asked, on the header line: it is short, and it is the part
            that says what actually happened. The answer is what needs folding. */}
        {!thinking && (
          <span className="step-args">{JSON.stringify(step.arguments)}</span>
        )}
        {!thinking && step.writes && (
          <span className="step-writes">{t('chat.changed')}</span>
        )}
      </button>
      {open && body && <pre className="step-body">{body}</pre>}
    </div>
  )
}

/** The steps of a turn, live or as they were recorded. */
function Steps({ steps }: { steps: RunStep[] }) {
  return (
    <>
      {steps.map((step, i) => (
        <Step key={i} step={step} />
      ))}
    </>
  )
}

function Turn({ message }: { message: Message }) {
  const t = useT()
  const meta = message.meta as
    | { model?: string; truncated?: boolean; stopped?: boolean; trace?: RunStep[] }
    | undefined

  return (
    <div className="bubble" data-role={message.role}>
      <div className="bubble-role">
        {t(`chat.role.${message.role}`)}
        {meta?.model && <span className="bubble-model">{meta.model}</span>}
      </div>

      {meta?.trace && <Steps steps={meta.trace} />}
      {message.content && <div className="bubble-body">{message.content}</div>}
      {meta?.stopped && <div className="bubble-note">{t('chat.stopped')}</div>}
      {meta?.truncated && !meta.stopped && (
        <div className="bubble-note">{t('chat.truncated')}</div>
      )}
    </div>
  )
}

/**
 * The turn currently being produced.
 *
 * Rendered from the run rather than from stored messages, and only while one is
 * going: the moment it finishes the server has written it down as an ordinary
 * message and this disappears, so there is never a window where the same turn
 * is on screen twice.
 */
function LiveTurn({ run, onCancel }: { run: RunState; onCancel: () => void }) {
  const t = useT()
  return (
    <div className="bubble" data-role="assistant" data-live="">
      <div className="bubble-role">
        {t('chat.role.assistant')}
        <span className="bubble-working">{t('chat.working')}</span>
        <span className="spacer" />
        <button className="link" onClick={onCancel}>
          {t('chat.stop')}
        </button>
      </div>
      <Steps steps={run.steps} />
      {run.steps.length === 0 && <div className="turn-text dim">{t('chat.thinkingNow')}</div>}
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
  const [sending, setSending] = useState(false)
  // Whether a turn is going is the *server's* fact, not this component's: it
  // survives a reload, and a second tab watching the same conversation must
  // agree with the first.
  const run = useStore((st) => (conversation ? st.runs[conversation] : undefined))
  const cancelRun = useStore((st) => st.cancelRun)
  const busy = sending || !!run?.running
  const tail = useRef<HTMLDivElement>(null)

  useEffect(() => {
    tail.current?.scrollIntoView({ block: 'end' })
  }, [messages.length, conversation, run?.steps.length])

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
    setSending(true)
    setDraft('')
    try {
      await sendMessage(text)
    } finally {
      setSending(false)
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
        {run?.running && <LiveTurn run={run} onCancel={() => void cancelRun()} />}
        {run?.error && <div className="bubble-note">{run.error}</div>}
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
