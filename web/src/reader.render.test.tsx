/**
 * The reader's chrome, rendered.
 *
 * pdf.js cannot run under jsdom, so the page canvas is out of reach — but
 * everything around it is exactly where a dead interaction hides: the file
 * switcher, the highlight palette, and the list of annotations, which is the
 * only part of a marked-up paper that is readable without the paper.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Item } from './api/types'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

const destroyed: string[][] = []

const child = (key: string, itemType: string, fields: Record<string, unknown>) =>
  ({
    key,
    libraryId: 1,
    itemType,
    parentKey: 'PAPER111',
    creators: [],
    tags: [],
    collections: [],
    version: 1,
    deleted: false,
    dateAdded: 0,
    dateModified: 0,
    ...fields,
  }) as unknown as Item

const marginNote = child('ANNO2222', 'annotation', {
  annotationType: 'note',
  annotationText: '',
  annotationComment: 'worth rereading before the meeting',
  annotationColor: 'amber',
  annotationPage: '8',
  annotationPosition: '{"page":8,"rects":[{"x":0.1,"y":0.3,"w":0.2,"h":0.02}]}',
})

const children = [
  child('FILE1111', 'attachment', { title: 'paper.pdf', filename: 'paper.pdf' }),
  child('ANNO1111', 'annotation', {
    annotationType: 'highlight',
    annotationText: 'attention is all you need',
    annotationColor: 'green',
    annotationPage: '7',
    annotationPosition: '{"page":7,"rects":[{"x":0.1,"y":0.2,"w":0.3,"h":0.02}]}',
  }),
]

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: (_t, _this, args: unknown[]) => {
        if (path === 'api.items.children') {
          const key = String(args[1] ?? '')
          // The attachment's children are its annotations; the paper's are its
          // files. Answering both from one list would hide a wrong call.
          return Promise.resolve(
            key === 'FILE1111' ? [children[1], marginNote] : [children[0]],
          )
        }
        if (path === 'api.items.destroy') {
          destroyed.push(args[1] as string[])
          return Promise.resolve({})
        }
        if (path === 'api.files.url') return 'blob:paper.pdf'
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

let container: HTMLElement
let root: Root

beforeEach(() => {
  destroyed.length = 0
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [{ id: 'reader:PAPER111', kind: 'reader', title: 'A paper', target: 'PAPER111' }],
    activeTab: 'reader:PAPER111',
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
  // Two hops: the attachment list, then that attachment's annotations.
  await act(async () => {
    await Promise.resolve()
  })
  await act(async () => {
    await Promise.resolve()
  })
}

describe('the reader', () => {
  it('lists the annotations on the open file', async () => {
    await render()

    const cards = container.querySelectorAll('.note-card')
    expect(cards).toHaveLength(2)
    expect(cards[0]?.textContent).toContain('attention is all you need')
  })

  it('keeps the colour the highlight was made in', async () => {
    await render()

    // A highlight's colour is how its author sorted it; showing them all in one
    // colour discards that silently.
    expect(container.querySelector('.note-card')?.getAttribute('data-colour')).toBe('green')
  })

  it('says which page a highlight is on', async () => {
    await render()

    expect(container.querySelector('.note-page')?.textContent).toContain('7')
  })

  it('shows a margin note, which highlights nothing and is only a comment', async () => {
    await render()

    // Imported Zotero notes have no quoted passage at all. Rendering only the
    // passage made every one of them an empty card.
    const cards = [...container.querySelectorAll('.note-card')]
    const note = cards.find((c) => c.textContent?.includes('worth rereading'))
    expect(note).toBeTruthy()
    expect(note?.querySelector('.note-comment')?.textContent).toBe(
      'worth rereading before the meeting',
    )
  })

  it('offers the whole palette to highlight with', async () => {
    await render()

    expect(container.querySelectorAll('.swatch').length).toBeGreaterThan(1)
  })

  it('lets a colour be chosen before there is anything to highlight', async () => {
    await render()
    const swatches = [...container.querySelectorAll('.swatch')] as HTMLElement[]
    const target = swatches.find((s) => s.getAttribute('data-colour') === 'blue')!

    await act(async () => {
      target.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })

    expect(target.getAttribute('data-active')).toBe('true')
  })

  it('tells the reader the page is still loading rather than showing nothing', async () => {
    await render()

    // pdf.js never resolves here, which is the same state as a slow disk: the
    // reader must not be left looking at a blank pane wondering.
    expect(container.querySelector('.reader-pages')?.textContent?.trim()).not.toBe('')
  })
})
