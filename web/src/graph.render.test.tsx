/**
 * The relationship graph, rendered.
 *
 * The layout is unit-tested beside itself, which says nothing about whether the
 * picture appears: the view measures its own pane, and jsdom lays nothing out,
 * so a graph that works in a browser can render zero nodes here — the same
 * shape of bug the virtualised table had. Only rendering it catches that.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { GraphNeighbourhood } from './api/types'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

const neighbourhood: GraphNeighbourhood = {
  focus: 'FOCUS111',
  nodes: [
    { key: 'FOCUS111', title: 'Attention is all you need', itemType: 'journalArticle', focus: true },
    { key: 'NEIGH222', title: 'Layer normalisation', year: 2016, itemType: 'journalArticle' },
    { key: 'NEIGH333', title: 'Adam', year: 2015, itemType: 'conferencePaper' },
  ],
  edges: [
    { source: 'FOCUS111', target: 'NEIGH222', relation: 'tag', weight: 2 },
    { source: 'FOCUS111', target: 'NEIGH333', relation: 'similar', weight: 0.81 },
  ],
}

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: () => {
        if (path === 'api.graph.around') return Promise.resolve(neighbourhood)
        if (path === 'api.items.get')
          return Promise.resolve({
            key: 'NEIGH222',
            libraryId: 1,
            itemType: 'journalArticle',
            title: 'Layer normalisation',
            creators: [],
            tags: [],
            collections: [],
            version: 1,
            deleted: false,
            dateAdded: 0,
            dateModified: 0,
          })
        // Everything else must not resolve, or bootstrap replaces the state
        // this test set up and the crash reads as a product bug.
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
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [
      {
        id: 'graph:FOCUS111',
        kind: 'graph',
        title: 'Attention is all you need',
        target: 'FOCUS111',
      },
    ],
    activeTab: 'graph:FOCUS111',
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
  // Let the neighbourhood request settle.
  await act(async () => {
    await Promise.resolve()
  })
}

describe('the graph tab', () => {
  it('draws a node for every item and an edge for every relationship', async () => {
    await render()

    expect(container.querySelectorAll('.graph-node')).toHaveLength(3)
    expect(container.querySelectorAll('.graph-edge')).toHaveLength(2)
  })

  it('places every node somewhere real', async () => {
    await render()

    // A NaN coordinate renders an invisible node rather than an error, which is
    // exactly how a broken force layout hides.
    for (const node of container.querySelectorAll('.graph-node')) {
      const transform = node.getAttribute('transform') ?? ''
      expect(transform).toMatch(/^translate\((-?[\d.]+) (-?[\d.]+)\)$/)
      expect(transform).not.toContain('NaN')
    }
  })

  it('marks the focus so it can be told from its neighbours', async () => {
    await render()

    const focused = container.querySelectorAll('.graph-node[data-focus]')
    expect(focused).toHaveLength(1)
    expect(focused[0]?.textContent).toContain('Attention is all you need')
  })

  it('says why each edge exists', async () => {
    await render()

    // An unexplained edge is a claim the reader has to take on trust.
    const relations = [...container.querySelectorAll('.graph-edge')].map((e) =>
      e.getAttribute('data-relation'),
    )
    expect(relations.sort()).toEqual(['similar', 'tag'])
    expect(container.querySelector('.graph-legend')).toBeTruthy()
  })

  it('reports its size in the status bar', async () => {
    await render()

    expect(useStore.getState().graphSize).toEqual({ nodes: 3, edges: 2 })
  })

  it('shows a neighbour that the list behind it does not contain', async () => {
    await render()

    // The table's selection model is an index into the visible list, and a
    // graph neighbour is deliberately not in it. Asking for one used to do
    // nothing whatsoever — the node highlighted, and that was all.
    const node = [...container.querySelectorAll('.graph-node')].find((n) =>
      n.textContent?.includes('Layer normalisation'),
    )!
    await act(async () => {
      node.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    await act(async () => {
      await Promise.resolve()
    })

    expect(useStore.getState().selected).toEqual(['NEIGH222'])
    expect(useStore.getState().detached?.title).toBe('Layer normalisation')
    expect(container.textContent).toContain('Layer normalisation')
  })
})

describe('a cited work the library does not hold', () => {
  beforeEach(() => {
    neighbourhood.nodes.push({
      key: 'doi:101000abc',
      title: 'A paper nobody here owns',
      itemType: 'journalArticle',
      external: true,
    })
    neighbourhood.edges.push({
      source: 'FOCUS111',
      target: 'doi:101000abc',
      relation: 'cites',
      weight: 1,
    })
  })

  afterEach(() => {
    neighbourhood.nodes.pop()
    neighbourhood.edges.pop()
  })

  it('is drawn, because what is missing is the point', async () => {
    await render()

    // A work cited by several papers on the shelf and owned by none is, almost
    // by definition, the next thing to read.
    const external = container.querySelectorAll('.graph-node[data-external]')
    expect(external).toHaveLength(1)
    expect(external[0]?.textContent).toContain('A paper nobody here owns')
  })

  it('cannot be selected, because there is nothing to show', async () => {
    await render()
    const external = container.querySelector('.graph-node[data-external]') as HTMLElement

    await act(async () => {
      external.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    await act(async () => {
      await Promise.resolve()
    })

    expect(useStore.getState().selected).toEqual([])
  })
})
