/**
 * The download queue, rendered.
 *
 * A queue is read by scanning it, so the tests are mostly about shape: a row
 * that wraps pushes every row under it out of alignment, and a list that
 * cannot be scanned is worse than one that cannot be read in full. The reason
 * a download failed still has to be reachable — on hover, in full.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Download } from './api/types'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

const retried: number[][] = []

let queue: Download[] = []

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: (_t, _this, args: unknown[]) => {
        if (path === 'api.downloads.list') {
          return Promise.resolve({
            downloads: queue,
            waiting: queue.filter((d) => d.state === 'waiting').length,
            failed: queue.filter((d) => d.state === 'failed').length,
          })
        }
        if (path === 'api.downloads.retry') {
          retried.push(args[1] as number[])
          return Promise.resolve({ requeued: (args[1] as number[]).length })
        }
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
  Element.prototype.getBoundingClientRect = () =>
    ({ width: 1200, height: 800, top: 0, left: 0, right: 1200, bottom: 800, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 800 })
})

let container: HTMLElement
let root: Root

beforeEach(() => {
  retried.length = 0
  queue = [
    {
      id: 1,
      itemKey: 'AAAA1111',
      url: 'https://example.org/paper.pdf',
      title: 'A paper',
      state: 'failed',
      error:
        'that address is a web page with no file linked from it, and the message keeps going for long enough to wrap',
      bytes: 0,
      attempts: 1,
    },
    {
      id: 2,
      itemKey: 'BBBB2222',
      url: 'https://example.org/other.pdf',
      title: 'Another paper',
      state: 'done',
      error: '',
      bytes: 2048,
      attempts: 1,
    },
  ] as unknown as Download[]

  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [{ id: 'downloads', kind: 'downloads', title: 'Downloads' }],
    activeTab: 'downloads',
    scopes: {},
    collections: [],
    smartCollections: [],
    tags: [],
    badgeDefs: [],
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
  await act(async () => {
    await Promise.resolve()
  })
}

const rows = () => [...container.querySelectorAll('.downloads-grid.row')]

describe('download queue', () => {
  it('lists what is queued', async () => {
    await render()
    expect(rows()).toHaveLength(2)
  })

  it('keeps a failure message on one line', async () => {
    await render()

    // The complaint that produced this test was that a long message wrapped,
    // which is the one thing a scannable list cannot afford.
    const error = container.querySelector('.download-error')
    expect(error).not.toBeNull()
    expect(error?.className).toContain('download-error')
  })

  it('keeps the whole message reachable', async () => {
    await render()

    // Truncating is only acceptable because the full text is a hover away.
    const error = container.querySelector('.download-error')
    expect(error?.getAttribute('title')).toContain('web page with no file linked')
  })

  it('offers a retry only where retrying means something', async () => {
    await render()

    const buttons = rows().map((r) => [...r.querySelectorAll('button')].map((b) => b.textContent))
    expect(buttons[0]?.some((label) => label?.includes('Retry'))).toBe(true)
    expect(buttons[1]?.some((label) => label?.includes('Retry'))).toBe(false)
  })
})
