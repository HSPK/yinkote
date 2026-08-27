/**
 * Typing in the search box.
 *
 * The bug this exists for: the input's value was read back out of the store,
 * and the store round trips a query through `parseQuery` and rejoins it with
 * single spaces. A trailing space does not survive that, so it was erased as
 * fast as it was typed and no multi-word search could be entered at all —
 * "attention is all you need" got as far as "attention".
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: () => new Promise(() => {}),
    })
  return { api: build('api'), connectEvents: () => () => {} }
})

beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as never
})

let container: HTMLElement
let root: Root

beforeEach(() => {
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [{ id: 'library', kind: 'library', title: 'Library' }],
    activeTab: 'library',
    scopes: {},
    collections: [],
    smartCollections: [],
    tags: [{ name: 'transformers', count: 3 } as never],
    badgeDefs: [],
    query: '',
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

const box = () => container.querySelector('#search-input') as HTMLInputElement

/** Type a string one character at a time, as a person would. */
async function type(text: string) {
  for (const ch of text) {
    const next = box().value + ch
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(
        window.HTMLInputElement.prototype,
        'value',
      )!.set!
      setter.call(box(), next)
      box().dispatchEvent(new Event('input', { bubbles: true }))
    })
  }
}

describe('the search box', () => {
  it('accepts a phrase with spaces in it', async () => {
    await render()
    await type('attention is all you need')

    expect(box().value).toBe('attention is all you need')
    expect(useStore.getState().query).toBe('attention is all you need')
  })

  it('keeps a space the moment it is typed', async () => {
    // The exact failure: one word, one space, and the space is gone.
    await render()
    await type('attention ')
    expect(box().value).toBe('attention ')
  })

  it('still turns a finished operator into a chip', async () => {
    // The reason the input was derived from the store in the first place: an
    // operator moves out of the box and becomes a chip, and the two must never
    // show the same thing.
    await render()
    await type('tag:transformers ')

    expect(container.querySelectorAll('.chip-token').length).toBe(1)
    expect(box().value).toBe('')
    // The trailing space stays in the query on purpose: it is what records
    // that the operator is finished, and without it the chip is read back as
    // half-typed and jumps into the input again.
    expect(useStore.getState().query).toBe('tag:transformers ')
  })

  it('goes on taking free text after a chip', async () => {
    await render()
    await type('tag:transformers ')
    await type('attention is')

    expect(box().value).toBe('attention is')
    expect(useStore.getState().query).toBe('tag:transformers attention is')
  })

  it('shows a query set from somewhere else', async () => {
    // A saved search, the command palette, a tag clicked in the sidebar: the
    // input is not the only thing that writes the query.
    await render()
    await act(async () => {
      useStore.setState({ query: 'from elsewhere' })
    })
    expect(box().value).toBe('from elsewhere')
  })
})
