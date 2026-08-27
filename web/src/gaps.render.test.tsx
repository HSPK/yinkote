/**
 * Required reading, rendered.
 *
 * This surface exists to answer one question — what is my library standing on
 * that it does not own — so the tests are about whether it answers it: the
 * ranking, the count, and whether a work can be turned into a library item in
 * one gesture.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { MissingWork } from './api/types'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

const added: string[] = []

let works: MissingWork[] = []

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: (_t, _this, args: unknown[]) => {
        if (path === 'api.references.missing') return Promise.resolve({ works })
        if (path === 'api.scrape.quickAdd') {
          const body = args[1] as { text: string }
          added.push(body.text)
          works = works.filter((w) => w.doi !== body.text)
          return Promise.resolve({ created: [{ title: 'Fetched paper' }] })
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

let container: HTMLElement
let root: Root

beforeEach(() => {
  added.length = 0
  works = [
    {
      fingerprint: 'doi:10 1016 j cell 2020 01 001',
      doi: '10.1016/j.cell.2020.01.001',
      label: 'Everyone leans on this',
      year: 2020,
      citedBy: 7,
    },
    {
      fingerprint: 'doi:10 1000 obscure',
      doi: '10.1000/obscure',
      label: 'Only one paper cites this',
      year: 2011,
      citedBy: 1,
    },
  ]

  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [{ id: 'gaps', kind: 'gaps', title: 'Required reading' }],
    activeTab: 'gaps',
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

const rows = () => [...container.querySelectorAll('.gaps-grid.row')]

describe('required reading', () => {
  it('lists what the library cites and does not hold', async () => {
    await render()

    expect(rows()).toHaveLength(2)
    expect(rows()[0]?.textContent).toContain('Everyone leans on this')
  })

  it('says how many of your own papers lean on each one', async () => {
    await render()

    // The count is the whole ranking: seven of your papers citing something you
    // have never read is a different fact from one paper citing it.
    expect(rows()[0]?.textContent).toContain('7')
  })

  it('reports its size in the status bar', async () => {
    await render()
    expect(useStore.getState().gapCount).toBe(2)
  })

  it('adds a work with the identifier the publisher wrote', async () => {
    await render()
    const button = rows()[0]?.querySelector('button') as HTMLElement

    await act(async () => {
      button.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    await act(async () => {
      await Promise.resolve()
    })

    // Not the fingerprint: that flattens punctuation and cannot be turned back
    // into a DOI, which is exactly what this row would need to fetch.
    expect(added).toEqual(['10.1016/j.cell.2020.01.001'])
  })

  it('drops a work from the list once the library holds it', async () => {
    await render()
    const button = rows()[0]?.querySelector('button') as HTMLElement

    await act(async () => {
      button.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    await act(async () => {
      await Promise.resolve()
    })
    await act(async () => {
      await Promise.resolve()
    })

    // Leaving the list is the only confirmation worth showing: the row was a
    // gap, and it is not one any more.
    expect(rows().map((r) => r.textContent)).not.toContain(
      expect.stringContaining('Everyone leans on this'),
    )
  })

  it('says what to do when there is nothing yet', async () => {
    works = []
    await render()

    // An empty list here means "you have not fetched any references", not "your
    // library is complete", and saying the wrong one would be a lie.
    expect(container.textContent).toContain('fetch the references')
  })
})
