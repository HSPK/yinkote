/**
 * Quick add, when nothing comes back.
 *
 * Pasting a link that yields no item has three quite different causes, and
 * they need opposite responses from the user: a publisher that refuses this
 * machine (use the browser connector), a source that is briefly down (wait),
 * and a work that simply is not there (check the link). For a long time all
 * three produced the same sentence, so the message was right by luck at best.
 *
 * These tests are about the *choice* of message, not its wording — they assert
 * against the catalogue, so translating a string cannot break them.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { ScrapeProblem, Unresolved } from './api/types'
import { enUS } from './i18n/en-US'
import { libraryTab } from './lib/tabs'
import { emptyScope } from './state/scope'
import { useOverlays } from './ui/overlays'
import { useStore } from './state/store'

let unresolved: Unresolved[] = []

const problem = (p: ScrapeProblem): Unresolved => ({
  kind: 'url',
  identifier: 'https://paywalled.example/article/1',
  problem: p,
  detail: 'forbidden: 403 Forbidden',
})

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: () => {
        if (path === 'api.scrape.quickAdd') {
          // Nothing resolved is an outcome, not an error: the server answers
          // 200 and says why, so the client can pick the right sentence.
          return Promise.resolve({ created: [], duplicates: [], unresolved, version: 1 })
        }
        if (path === 'api.items.list') return Promise.resolve({ items: [], total: 0 })
        if (path === 'api.collections.list') return Promise.resolve({ collections: [] })
        if (path === 'api.tags.list') return Promise.resolve({ tags: [] })
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
  }
})

let root: Root
let host: HTMLElement

beforeEach(() => {
  unresolved = []
  // Toasts live in a store that outlives the component tree, so one test's
  // message would otherwise still be on screen during the next.
  useOverlays.setState({ toasts: [] })
  host = document.createElement('div')
  document.body.append(host)
  // Boot is not what these tests are about; seed the state it produces.
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [libraryTab('Library')],
    activeTab: 'library',
    scopes: {},
    collections: [],
    smartCollections: [],
    tags: [],
    badgeDefs: [],
    selected: [],
  })
})

afterEach(() => {
  act(() => root.unmount())
  host.remove()
})

/** Paste `text` into the quick-add box and submit it. */
async function quickAdd(text: string) {
  await act(async () => {
    root = createRoot(host)
    root.render(<App />)
  })
  const box = host.querySelector<HTMLInputElement>('.quick-add input')
  expect(box, 'the quick-add box is not on the page').toBeTruthy()
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      'value',
    )!.set!
    setter.call(box!, text)
    box!.dispatchEvent(new Event('input', { bubbles: true }))
  })
  // The box submits on Enter; there is no form element around it.
  await act(async () => {
    box!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
  })
  await act(async () => {
    await Promise.resolve()
  })
}

describe('quick add says which kind of nothing it got', () => {
  it('names the publisher, and the way through, when refused', async () => {
    unresolved = [problem('blocked')]
    await quickAdd('https://paywalled.example/article/1')

    const text = host.textContent ?? ''
    expect(text).toContain(enUS['quickAdd.blocked'])
    // The hint is the actionable half: without it the user is told they are
    // blocked and nothing about the connector that would get through.
    expect(text).toContain(enUS['quickAdd.blockedHint'])
    expect(text).not.toContain(enUS['quickAdd.noMetadata'])
  })

  it('distinguishes a source being down from a work not existing', async () => {
    unresolved = [problem('unavailable')]
    await quickAdd('https://slow.example/a')

    const text = host.textContent ?? ''
    expect(text).toContain(enUS['quickAdd.unavailable'])
    expect(text).not.toContain(enUS['quickAdd.blocked'])
  })

  it('still says plainly when the work is simply not there', async () => {
    unresolved = [problem('notFound')]
    await quickAdd('10.9999/nope')

    expect(host.textContent ?? '').toContain(enUS['quickAdd.noMetadata'])
  })
})
