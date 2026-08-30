import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { useT } from '../i18n'
import type { Item, Message, RunState, RunStep } from '../api/types'
import { MentionPicker, mentionQuery, stripMention } from '../components/MentionPicker'
import { JumpRail, railMarks } from '../components/JumpRail'
import { VirtualList } from '../components/VirtualList'
import { agentProblem, elapsed } from '../lib/format'
import { type ScrollRequest, shouldFollow, type Tail } from '../lib/follow'
import { Markdown } from '../lib/markdown'
import { useStore } from '../state/store'
import { Empty, Icon, Select } from '../ui'

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
      {open && body && (
        <>
          <pre className="step-body">{body}</pre>
          {/* Said out loud: a reader must not take a cut answer for the whole
              of what a tool returned. */}
          {!thinking && step.clipped && <div className="step-clipped">{t('chat.clipped')}</div>}
        </>
      )}
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
/** How long the turn has been going.
 *
 *  Ticked here rather than taken from the server: progress arrives only every
 *  hundred milliseconds and stops altogether while the model is thinking,
 *  which is exactly when somebody wants to know how long they have waited.
 *  The server sends when it started, so a reload shows the real age instead
 *  of restarting the clock.
 */
function Elapsed({ since }: { since?: number }) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(timer)
  }, [])

  if (!since) return null
  return <span className="bubble-elapsed">{elapsed(now - since)}</span>
}

function LiveTurn({ run, onCancel }: { run: RunState; onCancel: () => void }) {
  const t = useT()
  return (
    <div className="bubble" data-role="assistant" data-live="">
      <div className="bubble-role">
        {t('chat.role.assistant')}
        <span className="bubble-working">{t('chat.working')}</span>
        <Elapsed since={run.startedAt} />
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

/** Everything the log draws, in order.
 *
 *  The live turn used to sit outside the scroller; inside it, the virtualiser
 *  measures it like anything else and it cannot overlap the message above.
 */
type Entry =
  | { kind: 'message'; id: string; message: Message }
  | { kind: 'live'; id: string; run: RunState }
  | { kind: 'error'; id: string; error: string; problem?: string }
  /** A marker at the top of what is loaded; asking for it fetches more. */
  | { kind: 'older'; id: string }

export function ChatView() {
  const agent = useStore((s) => s.agent)
  const t = useT()
  const conversation = useStore((s) => s.conversation)
  const conversations = useStore((s) => s.conversations)
  const messages = useStore((s) => s.messages)
  const sendMessage = useStore((s) => s.sendMessage)
  const retry = useStore((s) => s.retry)
  const [draft, setDraft] = useState('')
  const [sending, setSending] = useState(false)
  // Papers named with `@`, kept beside the text rather than inside it: the
  // assistant is told which item was meant instead of searching for a title
  // the user may have half-typed.
  const [mentions, setMentions] = useState<Item[]>([])
  const [mention, setMention] = useState<{ query: string; caret: number } | null>(null)
  const [focused, setFocused] = useState(false)
  const box = useRef<HTMLTextAreaElement>(null)
  // Whether a turn is going is the *server's* fact, not this component's: it
  // survives a reload, and a second tab watching the same conversation must
  // agree with the first.
  const run = useStore((st) => (conversation ? st.runs[conversation] : undefined))
  const cancelRun = useStore((st) => st.cancelRun)
  const hasOlder = useStore((st) => st.hasOlder)
  const loadingOlder = useStore((st) => st.loadingOlder)
  const loadOlder = useStore((st) => st.loadOlder)
  const collections = useStore((st) => st.collections)
  const setConversationScope = useStore((st) => st.setConversationScope)
  const busy = sending || !!run?.running

  /** Which entry to bring into view: the tail as it grows, or a rail jump. */
  const [jumpTo, setJumpTo] = useState<ScrollRequest | undefined>(undefined)
  // Each jump is its own request, so that "go to the bottom" still means
  // something when the bottom is the row it was last time — which is exactly
  // what happens while an answer streams into the live turn.
  const requests = useRef(0)
  const jump = useCallback(
    (index: number) => setJumpTo({ index, token: ++requests.current }),
    [],
  )
  const [firstVisible, setFirstVisible] = useState(0)

  // One list of things to draw. The live turn and an error are entries like
  // any other, so the virtualiser measures them too rather than having them
  // float outside it and overlap the last message.
  const entries = useMemo<Entry[]>(() => {
    const out: Entry[] = messages.map((message) => ({
      kind: 'message',
      id: `m${message.id}`,
      message,
    }))
    if (hasOlder) out.unshift({ kind: 'older', id: 'older' })
    if (run?.running) out.push({ kind: 'live', id: 'live', run })
    if (run?.error)
      out.push({ kind: 'error', id: 'error', error: run.error, problem: run.errorProblem })
    return out
  }, [messages, run])

  // Offset by the "load older" marker when there is one, since the rail
  // addresses list positions and the marker occupies the first.
  const offset = hasOlder ? 1 : 0
  const marks = useMemo(
    () => railMarks(messages).map((m) => ({ ...m, index: m.index + offset })),
    [messages, offset],
  )

  // Follow the tail — but only when the tail actually moved. Loading earlier
  // messages makes the list longer at the *other* end, and following the
  // count there would yank the reader to the bottom of a thread they were
  // deliberately scrolling back through. See `shouldFollow`.
  const seen = useRef<Tail | null>(null)
  const tail: Tail = {
    id: entries[entries.length - 1]?.id,
    steps: run?.steps?.length ?? 0,
  }
  useEffect(() => {
    if (shouldFollow(seen.current, tail)) {
      if (entries.length) jump(entries.length - 1)
    }
    seen.current = tail
    // Depends on the tail's identity, not on the array: a re-render with the
    // same last entry must not scroll anything.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tail.id, tail.steps, entries.length])

  // A different conversation starts at its own bottom.
  useEffect(() => {
    seen.current = null
  }, [conversation])

  /** Load earlier messages without moving what the reader is looking at.
   *
   *  Content added above pushes everything down, so the anchor is the message
   *  that was at the top: after the page arrives, that same message is put
   *  back under the pointer. Without this, asking for history throws you into
   *  the middle of it.
   */
  const loadEarlier = async () => {
    const anchor = messages[0]?.id
    await loadOlder()
    if (anchor === undefined) return
    const at = useStore.getState().messages.findIndex((m) => m.id === anchor)
    if (at >= 0) {
      // +1 for the marker that still sits above, when there is more still.
      jump(at + (useStore.getState().hasOlder ? 1 : 0))
    }
  }

  if (!conversation) {
    return (
      <div className="pane main">
        <Empty>{t('chat.none')}</Empty>
      </div>
    )
  }

  const current = conversations.find((c) => c.key === conversation)
  const title = current?.title || t('chat.untitled')
  const scope = current?.scope

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
      <div className="chat-head">
        <span className="chat-title">{title}</span>
      </div>

      {/* The log is virtualised because a conversation is not bounded: a
          working thread runs to hundreds of messages, and a tool trace can be
          hundreds of lines on its own. Heights are measured rather than
          assumed, since a one-line answer and a bibliography dump sit next to
          each other. */}
      <div className="chat-body">
        <VirtualList
          rows={entries}
          keyOf={(entry) => entry.id}
          className="chat-log"
          dynamic
          rowHeight={90}
          scrollTo={jumpTo}
          empty={<Empty>{t('chat.start')}</Empty>}
          onVisibleChange={setFirstVisible}
        >
          {(entry) =>
            entry.kind === 'message' ? (
              <Turn message={entry.message} />
            ) : entry.kind === 'live' ? (
              <LiveTurn run={entry.run} onCancel={() => void cancelRun()} />
            ) : entry.kind === 'older' ? (
              <button
                className="chat-older"
                disabled={loadingOlder}
                onClick={() => void loadEarlier()}
              >
                {loadingOlder ? t('chat.loadingOlder') : t('chat.loadOlder')}
              </button>
            ) : (
              // The class of failure from the catalogue; the server's own
              // words stay on the element, which is where they belong. A
              // throttled model used to put raw JSON in the chat.
              //
              // And a way to act on it. "Try again in a moment" with nothing
              // to try again *with* made the reader retype the question --
              // and a long one, typed into a box that has just lost it, is a
              // question people abandon.
              <div className="bubble-note" title={entry.error}>
                <span>{agentProblem(t, entry.problem) || entry.error}</span>
                <button className="bubble-retry" onClick={() => void retry()}>
                  {t('chat.retry')}
                </button>
              </div>
            )
          }
        </VirtualList>

        {marks.length > 0 && <JumpRail marks={marks} active={firstVisible} onJump={jump} />}
      </div>

      {/* One control, not three stacked. What the question is about — the
          papers named and the collection it is scoped to — belongs beside the
          text being typed, because those are all part of the question. */}
      <div className="chat-input">
        {mention && (
          <MentionPicker query={mention.query} onPick={pick} onDismiss={() => setMention(null)} />
        )}

        <div className="composer" data-focused={focused || undefined}>
          {mentions.length > 0 && (
            <div className="composer-chips">
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

          <textarea
            ref={box}
            value={draft}
            rows={2}
            placeholder={t('chat.mentionHint')}
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
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

          <div className="composer-bar">
            {/* The shared control, not a bare `<select>`: the browser's
                default is a white box in a dark interface, and every other
                select in the program already knows what it should look like. */}
            <label className="chat-scope" title={t('chat.scope')}>
              <Icon.Folder size={11} className="glyph" />
              <Select
                value={scope ?? ''}
                onChange={(e) => void setConversationScope(e.target.value || null)}
                options={[
                  { value: '', label: t('chat.scopeAll') },
                  ...collections.map((c) => ({ value: c.key, label: c.name })),
                ]}
              />
            </label>

            <span className="spacer" />

            {/* Without a model the turn is still recorded — a thread against a
                paper is worth keeping — but nothing will ever answer it. Saying
                so beside the button is the difference between a feature that is
                off and one that looks broken; `chat.ts` claimed the box
                "explains itself if not", and it did not. */}
            {agent && !agent.configured && (
              <span className="chat-nomodel" title={t('chat.noModelHint')}>
                {t('chat.noModel')}
              </span>
            )}

            <button
              className="primary"
              disabled={!draft.trim() || busy}
              onClick={() => void submit()}
            >
              {t('chat.send')}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
