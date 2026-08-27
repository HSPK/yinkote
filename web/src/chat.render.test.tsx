/**
 * The chat surface, rendered.
 *
 * An agent answer is built from searches the reader cannot see, so the parts
 * that make it checkable — which tools ran, which model answered, whether the
 * transcript was cut — are the parts most worth a test. They are also the
 * easiest to lose silently, since a missing footnote still looks like an
 * answer.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Conversation, Message } from './api/types'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

const sent: string[] = []

let holdMessages = false
/** What a request for earlier messages answers with. */
let olderPage: { messages: unknown[]; hasMore: boolean } | null = null
/** What the mention picker will find. */
let pickable: unknown[] = []
/** Mentions carried by the last send. */
const sentMentions: string[][] = []
/** Scopes set on the conversation. */
const scoped: (string | null)[] = []

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: (_t, _this, args: unknown[]) => {
        // Without a model the workbench appends the turn rather than asking;
        // both paths carry the text as the third argument.
        if (path === 'api.conversations.ask') {
          sent.push(String(args[2] ?? ''))
          sentMentions.push((args[3] as string[] | undefined) ?? [])
          return Promise.resolve({})
        }
        if (path === 'api.conversations.setScope') {
          scoped.push(args[2] as string | null)
          return Promise.resolve({ key: 'K1', libraryId: 1, title: 'T', scope: args[2] })
        }
        if (path === 'api.items.list') {
          return Promise.resolve({ items: pickable, total: pickable.length })
        }
        if (path === 'api.conversations.append') {
          const body = args[2] as { content?: string; mentions?: string[] } | undefined
          sent.push(String(body?.content ?? ''))
          sentMentions.push(body?.mentions ?? [])
          return Promise.resolve({})
        }
        if (path === 'api.conversations.messages') {
          // Held open to stand in for the round trip that fetches the stored
          // copy of an answer.
          if (holdMessages) return new Promise(() => {})
          const opts = args[2] as { before?: number } | undefined
          if (opts?.before !== undefined && olderPage) return Promise.resolve(olderPage)
          return Promise.resolve({ messages: [], hasMore: false })
        }
        if (path === 'api.conversations.run') return Promise.resolve({ running: false })
        if (path === 'api.conversations.list') return Promise.resolve([])
        return new Promise(() => {})
      },
    })
  return { api: build('api'), connectEvents: () => () => {} }
})

beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as never
  Element.prototype.scrollIntoView = () => {}
  Element.prototype.getBoundingClientRect = () =>
    ({
      width: 1200,
      height: 800,
      top: 0,
      left: 0,
      right: 1200,
      bottom: 800,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    }) as DOMRect
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 800 })
})

const message = (id: number, role: string, content: string, meta?: unknown) =>
  ({ id, conversationKey: 'K1', role, content, meta, createdAt: 0 }) as unknown as Message

let container: HTMLElement
let root: Root

beforeEach(() => {
  sent.length = 0
  sentMentions.length = 0
  scoped.length = 0
  olderPage = null
  pickable = [
    {
      key: 'PAPER001',
      libraryId: 1,
      itemType: 'journalArticle',
      title: 'Attention Is All You Need',
      creators: [{ creatorType: 'author', lastName: 'Vaswani' }],
      date: '2017',
      tags: [],
      collections: [],
      version: 1,
      deleted: false,
      dateAdded: 0,
      dateModified: 0,
    },
  ]
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [{ id: 'chat', kind: 'chat', title: 'Chat' }],
    activeTab: 'chat',
    scopes: {},
    collections: [],
    smartCollections: [],
    tags: [],
    badgeDefs: [],
    conversation: 'K1',
    // Reset explicitly: a live run left behind by an earlier test makes the
    // composer think a turn is in flight, and every send silently does
    // nothing. State that leaks between tests eventually tests the leak.
    runs: {},
    asking: false,
    conversations: [
      { key: 'K1', libraryId: 1, title: 'What did I read about attention?' } as Conversation,
    ],
    messages: [
      message(1, 'user', 'What did I read about attention?'),
      message(2, 'assistant', 'Three papers, all from 2017.', {
        model: 'gpt-5.6-sol',
        trace: [
          { kind: 'thinking', content: 'The user wants papers about attention.' },
          { kind: 'text', content: 'Let me look.' },
          {
            kind: 'tool',
            name: 'search_library',
            arguments: { query: 'attention' },
            result: '{"count":3}',
            writes: false,
          },
          {
            kind: 'tool',
            name: 'tag_items',
            arguments: { keys: ['AAAA1111'], tags: ['read'] },
            result: '{"changed":1}',
            writes: true,
          },
        ],
      }),
    ],
  })
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

async function render() {
  await act(async () => {
    root.render(<App />)
  })
}

/** React ignores an input event whose value it believes it already knows. */
function type(field: HTMLTextAreaElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(
    HTMLTextAreaElement.prototype,
    'value',
  )!.set!
  setter.call(field, value)
  field.dispatchEvent(new Event('input', { bubbles: true }))
}

describe('the chat surface', () => {
  it('shows the conversation and both sides of it', async () => {
    await render()

    expect(container.textContent).toContain('What did I read about attention?')
    expect(container.querySelectorAll('.bubble')).toHaveLength(2)
  })

  it('names the model that answered', async () => {
    await render()

    // Which model answered changes what the answer is worth, and it is the
    // first thing to look at when one turn is unlike the others.
    expect(container.querySelector('.bubble-model')?.textContent).toBe('gpt-5.6-sol')
  })

  it('draws each tool call where it happened, in order', async () => {
    await render()

    // A list of calls after the answer loses the thing that makes an answer
    // checkable: which step led to which.
    const parts = [...container.querySelectorAll('.bubble-body, .turn-text, .turn-step')].map(
      (n) => n.textContent ?? '',
    )
    const look = parts.findIndex((p) => p.includes('Let me look'))
    const search = parts.findIndex((p) => p.includes('search_library'))
    const answer = parts.findIndex((p) => p.includes('Three papers'))

    expect(look).toBeGreaterThanOrEqual(0)
    expect(search).toBeGreaterThan(look)
    expect(answer).toBeGreaterThan(search)
  })

  it('shows what was asked without unfolding anything', async () => {
    await render()

    // The arguments are short and are the part that says what was actually
    // asked; the result is a page of JSON and stays folded.
    // The header carries what was asked; the answer is what needs folding, and
    // nothing is unfolded until somebody asks.
    expect(container.querySelector('.step-args')?.textContent).toContain('attention')
    expect(container.querySelector('.step-body')).toBeNull()
  })

  it('marks the step that changed the library', async () => {
    await render()

    // "Which of these actually did something" is the first question anybody
    // asks of a trace, and an agent with write access must answer it.
    const written = container.querySelectorAll('.turn-step[data-writes]')
    expect(written).toHaveLength(1)
    expect(written[0]?.textContent).toContain('tag_items')
  })

  it('says when a transcript was cut rather than pretending it was whole', async () => {
    useStore.setState({
      messages: [message(3, 'assistant', 'Shortened.', { truncated: true })],
    })
    await render()

    expect(container.querySelector('.bubble-note')).toBeTruthy()
  })

  it('sends on Enter and clears the box', async () => {
    await render()
    const field = container.querySelector('.chat-input textarea') as HTMLTextAreaElement

    await act(async () => type(field, 'and what about diffusion?'))
    await act(async () => {
      field.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
      )
    })

    expect(sent).toEqual(['and what about diffusion?'])
    expect(field.value).toBe('')
  })

  it('does not send an empty message', async () => {
    await render()
    const field = container.querySelector('.chat-input textarea') as HTMLTextAreaElement

    await act(async () => type(field, '   '))
    await act(async () => {
      field.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
      )
    })

    expect(sent).toEqual([])
  })

  it('leaves shift-enter to the textarea, so a question can have paragraphs', async () => {
    await render()
    const field = container.querySelector('.chat-input textarea') as HTMLTextAreaElement

    await act(async () => type(field, 'first line'))
    await act(async () => {
      field.dispatchEvent(
        new KeyboardEvent('keydown', {
          key: 'Enter',
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      )
    })

    expect(sent).toEqual([])
    expect(field.value).toBe('first line')
  })
})

describe('a turn in flight', () => {
  it('keeps the header still when a step is opened', async () => {
    await render()
    const bar = container.querySelector('.step-bar') as HTMLElement

    // Nothing is unfolded to begin with, so opening one can only add below it.
    expect(container.querySelector('.step-body')).toBeNull()
    await act(async () => {
      bar.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })

    const bars = [...container.querySelectorAll('.step-bar')]
    expect(bars[0]).toBe(bar)
    expect(container.querySelector('.step-body')).toBeTruthy()
  })

  it('folds the model’s reasoning away rather than reading it as prose', async () => {
    await render()

    // Reasoning is working, not an answer. Showing it as prose would be
    // presenting a draft as a conclusion.
    const thinking = [...container.querySelectorAll('.step-name')].find(
      (n) => n.textContent === 'Thinking',
    )
    expect(thinking).toBeTruthy()
    expect(container.textContent).not.toContain('The user wants papers about attention.')
  })

  it('shows a running turn with a way to stop it', async () => {
    useStore.setState({
      runs: {
        K1: {
          running: true,
          question: 'and diffusion?',
          steps: [{ kind: 'text', content: 'Looking.' }],
          reply: '',
          truncated: false,
          stopped: false,
          error: null,
        },
      },
    })
    await render()

    const live = container.querySelector('.bubble[data-live]')
    expect(live).toBeTruthy()
    // A turn that cannot be interrupted is one you have to wait out.
    expect(live?.textContent).toContain('Stop')
  })
})

describe('a run state with holes in it', () => {
  it('does not blank the pane when steps are missing', async () => {
    // This is not hypothetical: the server used to answer `{"running": false}`
    // for a conversation with no turn — the same type with holes in it — and
    // the pane crashed on `steps.length`, which the reader saw as "this panel
    // failed to draw". Both ends were fixed; this holds the client's half.
    useStore.setState({
      runs: { K1: { running: true } as never },
    })
    await render()

    expect(container.querySelector('.bubble[data-live]')).toBeTruthy()
    expect(container.textContent).not.toContain('failed to draw')
  })
})

describe('an answer arriving', () => {
  const running = (extra: Record<string, unknown>) => ({
    runs: {
      K1: {
        running: true,
        question: 'q',
        steps: [],
        reply: '',
        truncated: false,
        stopped: false,
        error: null,
        ...extra,
      } as never,
    },
  })

  it('shows the answer as it arrives rather than after it', async () => {
    useStore.setState(running({ partial: 'Three papers, all' }))
    await render()

    // A turn that shows nothing for half a minute is indistinguishable from
    // one that has hung.
    expect(container.querySelector('.bubble[data-live]')?.textContent).toContain(
      'Three papers, all',
    )
  })

  it('previews reasoning on one line instead of unrolling it', async () => {
    useStore.setState(running({ partialReasoning: 'x'.repeat(400) }))
    await render()

    const preview = container.querySelector('.step-bar[data-static] .step-args')
    expect(preview).toBeTruthy()
    // Reasoning can run for pages; it must not push the answer off the screen.
    expect((preview?.textContent ?? '').length).toBeLessThan(120)
  })

  it('says how long the turn has been going', async () => {
    // Progress arrives every hundred milliseconds and stops altogether while
    // the model thinks — which is exactly when somebody wants to know how
    // long they have been waiting.
    useStore.setState(running({ partial: 'x', startedAt: Date.now() - 125_000 }))
    await render()
    expect(container.querySelector('.bubble-elapsed')?.textContent).toBe('2m 05s')
  })

  it('shows the real age of a turn rejoined after a reload', async () => {
    useStore.setState(running({ partial: 'x', startedAt: Date.now() - 9_000 }))
    await render()
    // Not "0s": the clock belongs to the turn, not to this page load.
    expect(container.querySelector('.bubble-elapsed')?.textContent).toBe('9s')
  })

  it('stops saying “thinking” once anything has arrived', async () => {
    useStore.setState(running({ partial: 'Here.' }))
    await render()
    expect(container.textContent).not.toContain('Thinking…')
  })

  it('never takes the answer off screen while swapping it for the stored one', async () => {
    useStore.setState(running({ partial: 'Three papers, all recent.' }))
    await render()
    expect(container.textContent).toContain('Three papers, all recent.')

    // The finish arrives on the event bus. The stored copy of the answer has
    // to be fetched, and that fetch is held open here — standing in for the
    // round trip during which the answer used to vanish and come back, which
    // is exactly what the flicker was.
    holdMessages = true
    await act(async () => {
      useStore.getState().applyRun('K1', {
        running: false,
        question: 'q',
        steps: [],
        reply: 'Three papers, all recent.',
        partial: 'Three papers, all recent.',
        truncated: false,
        stopped: false,
        error: null,
      })
    })

    expect(container.textContent).toContain('Three papers, all recent.')
    holdMessages = false
  })
})

describe('naming a paper in a question', () => {
  const box = () => container.querySelector('textarea')!

  /** Type into the composer the way a person does, caret and all. */
  async function type(text: string) {
    const el = box()
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(
        window.HTMLTextAreaElement.prototype,
        'value',
      )!.set!
      setter.call(el, text)
      el.selectionStart = text.length
      el.selectionEnd = text.length
      el.dispatchEvent(new Event('input', { bubbles: true }))
    })
    // The picker searches on a debounce.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 200))
    })
  }

  it('opens a list when an @ is typed', async () => {
    await render()
    await type('what about @atten')

    // Unit tests cover the parsing; only rendering shows whether the box is
    // actually wired to it.
    expect(container.querySelector('.mention-popup')).not.toBeNull()
    expect(container.querySelector('.mention-row')?.textContent).toContain('Attention')
  })

  it('does not open on an @ inside a word', async () => {
    await render()
    await type('mail bob@example.com')
    expect(container.querySelector('.mention-popup')).toBeNull()
  })

  it('attaches the paper as a chip and takes the @ out of the text', async () => {
    await render()
    await type('compare @atten')

    const row = container.querySelector('.mention-row') as HTMLElement
    await act(async () => {
      row.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
    })

    expect(container.querySelector('.mention-chip')?.textContent).toContain('Attention')
    expect(box().value).not.toContain('@atten')
  })

  it('sends the key rather than the text', async () => {
    await render()
    await type('compare @atten')
    const row = container.querySelector('.mention-row') as HTMLElement
    await act(async () => {
      row.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
    })

    await act(async () => {
      box().dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
      )
    })

    // The whole point: the assistant is told which paper was meant instead of
    // searching for a title the user half-typed.
    expect(sentMentions[0]).toEqual(['PAPER001'])
  })

  it('clears the chips once the question is sent', async () => {
    await render()
    await type('compare @atten')
    const row = container.querySelector('.mention-row') as HTMLElement
    await act(async () => {
      row.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
    })
    await act(async () => {
      box().dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
      )
    })
    // Otherwise the next unrelated question silently carries the same paper.
    expect(container.querySelector('.mention-chip')).toBeNull()
  })
})

describe('scoping a conversation', () => {
  it('offers the collections and records the choice', async () => {
    useStore.setState({
      collections: [
        { key: 'COLL0001', libraryId: 1, name: 'Diffusion', itemCount: 12 },
      ] as never,
    })
    await render()

    const select = container.querySelector('.chat-scope select') as HTMLSelectElement
    expect(select).not.toBeNull()
    expect(select.textContent).toContain('Diffusion')

    await act(async () => {
      select.value = 'COLL0001'
      select.dispatchEvent(new Event('change', { bubbles: true }))
    })
    expect(scoped).toEqual(['COLL0001'])
  })
})

describe('a long conversation', () => {
  const many = (n: number) =>
    Array.from({ length: n }, (_, i) =>
      message(i + 1, i % 2 === 0 ? 'user' : 'assistant', `Message ${i + 1}`),
    )

  it('draws a handful of messages, not four hundred', async () => {
    useStore.setState({ messages: many(400) })
    await render()

    // A working thread runs to hundreds of messages and a tool trace can be
    // hundreds of lines on its own; rendering all of it is what made opening
    // an old conversation slow.
    const drawn = container.querySelectorAll('.chat-log .bubble')
    expect(drawn.length).toBeGreaterThan(0)
    expect(drawn.length).toBeLessThan(60)
  })

  it('offers a rail once scrolling stops being enough', async () => {
    useStore.setState({ messages: many(400) })
    await render()

    // One tick per question: what somebody is looking for is something they
    // asked, and the answers between are what makes it hard to find.
    const ticks = container.querySelectorAll('.jump-tick')
    expect(ticks.length).toBe(200)
  })

  it('offers the earlier messages when the thread is longer than what is held', async () => {
    useStore.setState({ messages: many(40), hasOlder: true })
    await render()
    // Opening a conversation must not depend on how long it has been going,
    // so the top of what is loaded says there is more rather than pretending
    // the thread starts there.
    expect(container.querySelector('.chat-older')).not.toBeNull()
  })

  it('keeps the reader where they were after loading earlier messages', async () => {
    const older = many(20)
    olderPage = { messages: older, hasMore: false }
    useStore.setState({ messages: many(40).map((m) => ({ ...m, id: m.id + 100 })), hasOlder: true })
    await render()

    const before = container.querySelector('.chat-older') as HTMLButtonElement
    await act(async () => before.click())

    // Content added above pushes everything down. Following the count here —
    // the list did get longer — throws the reader to the bottom of exactly
    // the history they were scrolling back to read.
    expect(useStore.getState().messages).toHaveLength(60)
    expect(container.querySelector('.chat-older')).toBeNull()
  })

  it('says nothing about earlier messages when there are none', async () => {
    useStore.setState({ messages: many(40), hasOlder: false })
    await render()
    expect(container.querySelector('.chat-older')).toBeNull()
  })

  it('is there on a short conversation too', async () => {
    useStore.setState({ messages: many(4) })
    await render()
    // It used to hide below a dozen messages, which meant it appeared
    // unannounced part-way through a conversation. A control that comes and
    // goes is harder to rely on than one that is always in the same place.
    expect(container.querySelector('.jump-rail')).not.toBeNull()
    expect(container.querySelectorAll('.jump-tick')).toHaveLength(2)
  })

  it('shows nothing to aim at until a question has been asked', async () => {
    useStore.setState({ messages: [] })
    await render()
    expect(container.querySelector('.jump-rail')).toBeNull()
  })

  it('dates the question it previews', async () => {
    const at = new Date()
    at.setHours(9, 5, 0, 0)
    useStore.setState({
      messages: many(4).map((m) => ({ ...m, createdAt: at.getTime() })),
    })
    await render()

    const tick = container.querySelector('.jump-tick') as HTMLElement
    await act(async () => {
      tick.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }))
    })
    // Same day, so the date would be noise; two questions an hour apart are
    // still told apart.
    expect(container.querySelector('.jump-preview-time')?.textContent).toBe('09:05')
  })

  it('previews the question a tick stands for', async () => {
    useStore.setState({ messages: many(400) })
    await render()

    const tick = container.querySelector('.jump-tick') as HTMLElement
    await act(async () => {
      // React synthesises `onMouseEnter` from delegated `mouseover`; a
      // dispatched `mouseenter` does not bubble and never reaches it.
      tick.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }))
    })
    expect(container.querySelector('.jump-preview')?.textContent).toContain('Message 1')
  })
})
