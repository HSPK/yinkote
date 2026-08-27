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

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: (_t, _this, args: unknown[]) => {
        // Without a model the workbench appends the turn rather than asking;
        // both paths carry the text as the third argument.
        if (path === 'api.conversations.ask') {
          sent.push(String(args[2] ?? ''))
          return Promise.resolve({})
        }
        if (path === 'api.conversations.append') {
          sent.push(String((args[2] as { content?: string } | undefined)?.content ?? ''))
          return Promise.resolve({})
        }
        if (path === 'api.conversations.messages') {
          // Held open to stand in for the round trip that fetches the stored
          // copy of an answer.
          return holdMessages ? new Promise(() => {}) : Promise.resolve([])
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
