import { useEffect, useRef, useState } from 'react'

import { useT } from '../i18n'
import type { Item, Message, RunState, RunStep } from '../api/types'
import { MentionPicker, mentionQuery, stripMention } from '../components/MentionPicker'
import { Markdown } from '../lib/markdown'
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

/** The last words of a long fragment, for a one-line preview.
 *
 *  The end rather than the beginning: reasoning that is still arriving is most
 *  interesting where it currently is. */
function tail(text: string, chars = 90): string {
  const flat = text.replace(/\s+/g, ' ').trim()
  return flat.length > chars ? `…${flat.slice(-chars)}` : flat
}

/** The steps of a turn, live or as they were recorded. */
function Steps({ steps }: { steps?: RunStep[] }) {
  // Defensive on purpose. A state can arrive from an older server or a future
  // one, and a chat pane that blanks itself over a missing array is a worse
  // failure than one that shows nothing for a moment.
  return (
    <>
      {(steps ?? []).map((step, i) => (
        <Step key={i} step={step} />
      ))}
    </>
  )
}

/** A named paper, shown by title once the list has it. */
function MentionRef({ itemKey }: { itemKey: string }) {
  const openReader = useStore((s) => s.openReader)
  const title = useStore((s) => s.items.find((i) => i.key === itemKey)?.title)
  return (
    <button className="bubble-mention" onClick={() => openReader(itemKey)}>
      {title || itemKey}
    </button>
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

      {/* What the question was about, kept visible on the message. Reading a
          thread back, "this one" in the prose is meaningless without it. */}
      {!!message.mentions?.length && (
        <div className="bubble-mentions">
          {message.mentions.map((key) => (
            <MentionRef key={key} itemKey={key} />
          ))}
        </div>
      )}

      {meta?.trace && <Steps steps={meta.trace} />}
      {message.content && (
        <div className="bubble-body">
          <Markdown source={message.content} />
        </div>
      )}
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

      {/* What is arriving right now. Shown as ordinary text rather than in a
          step, because it is not finished being one — the moment it is, the
          step replaces it and the words do not move. */}
      {run.partialReasoning && (
        <div className="turn-step">
          <div className="step-bar" data-static="">
            <Icon.Bulb size={11} />
            <span className="step-name">{t('chat.thinking')}</span>
            <span className="step-args">{tail(run.partialReasoning)}</span>
          </div>
        </div>
      )}
      {run.partial && (
        <div className="bubble-body">
          <Markdown source={run.partial} />
        </div>
      )}
      {!run.steps?.length && !run.partial && !run.partialReasoning && (
        <div className="turn-text dim">{t('chat.thinkingNow')}</div>
      )}
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
  // Papers named with `@`, kept beside the text rather than inside it: the
  // assistant is told which item was meant instead of searching for a title
  // the user may have half-typed.
  const [mentions, setMentions] = useState<Item[]>([])
  const [mention, setMention] = useState<{ query: string; caret: number } | null>(null)
  const box = useRef<HTMLTextAreaElement>(null)
  // Whether a turn is going is the *server's* fact, not this component's: it
  // survives a reload, and a second tab watching the same conversation must
  // agree with the first.
  const run = useStore((st) => (conversation ? st.runs[conversation] : undefined))
  const cancelRun = useStore((st) => st.cancelRun)
  const busy = sending || !!run?.running
  const tail = useRef<HTMLDivElement>(null)

  useEffect(() => {
    tail.current?.scrollIntoView({ block: 'end' })
  }, [messages.length, conversation, run?.steps?.length])

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
    const named = mentions.map((m) => m.key)
    setMentions([])
    setMention(null)
    try {
      await sendMessage(text, named)
    } finally {
      setSending(false)
    }
  }

  const pick = (item: Item) => {
    const caret = mention?.caret ?? draft.length
    setDraft(stripMention(draft, caret))
    setMentions((current) =>
      current.some((m) => m.key === item.key) ? current : [...current, item],
    )
    setMention(null)
    box.current?.focus()
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
        {mentions.length > 0 && (
          <div className="mention-chips">
            <span className="dim">{t('chat.mentioned')}</span>
            {mentions.map((m) => (
              <button
                key={m.key}
                className="mention-chip"
                title={t('chat.removeMention')}
                onClick={() => setMentions((c) => c.filter((x) => x.key !== m.key))}
              >
                <span className="mention-chip-label">{m.title || m.key}</span>
                <Icon.Close size={8} />
              </button>
            ))}
          </div>
        )}
        {mention && (
          <MentionPicker
            query={mention.query}
            onPick={pick}
            onDismiss={() => setMention(null)}
          />
        )}
        <textarea
          ref={box}
          value={draft}
          rows={2}
          placeholder={t('chat.mentionHint')}
          onChange={(e) => {
            setDraft(e.target.value)
            const caret = e.target.selectionStart ?? e.target.value.length
            const query = mentionQuery(e.target.value, caret)
            setMention(query === null ? null : { query, caret })
          }}
          onKeyDown={(e) => {
            // While the picker is open it owns Enter; see MentionPicker.
            if (e.key === 'Enter' && !e.shiftKey && !mention) {
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
