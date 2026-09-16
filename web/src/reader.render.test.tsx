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
import { DEFAULT_READER_LAYOUT } from './lib/reader-layout'

const destroyed: string[][] = []
let attached: Item[] = []
const documentLoads: string[] = []
const startedDocuments: string[] = []
const closedDocuments: string[] = []
const pendingDocument = { doc: null, pages: [], error: null }

vi.mock('./pages/usePdf', async () => {
  const { useEffect } = await import('react')
  return {
    usePdf: (url: string) => {
      documentLoads.push(url)
      useEffect(() => {
        startedDocuments.push(url)
        return () => { closedDocuments.push(url) }
      }, [url])
      return pendingDocument
    },
  }
})

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
        if (path === 'api.items.get') {
          return Promise.resolve(args[1] === 'FILE1111'
            ? children[0] : child('PAPER111', 'journalArticle', { title: 'Paper' }))
        }
        if (path === 'api.items.children') {
          const key = String(args[1] ?? '')
          // The attachment's children are its annotations; the paper's are its
          // files. Answering both from one list would hide a wrong call.
          return Promise.resolve(
            key === 'FILE1111' ? [children[1], marginNote] : attached,
          )
        }
        if (path === 'api.items.destroy') {
          destroyed.push(args[1] as string[])
          return Promise.resolve({})
        }
        if (path === 'api.files.url') return `blob:${args[1]}`
        if (path === 'api.settings.put') return Promise.resolve({})
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
  attached = [children[0]!]
  documentLoads.length = 0
  startedDocuments.length = 0
  closedDocuments.length = 0
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
    readerLayout: { ...DEFAULT_READER_LAYOUT, notesOpen: true },
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
  it('keeps the PDF session mounted across tab changes and releases it on close', async () => {
    await render()
    const reader = container.querySelector('.reader')
    expect(reader).not.toBeNull()
    expect(startedDocuments).toEqual(['blob:FILE1111'])
    await act(async () => useStore.getState().openSettings())
    expect(container.querySelector('.reader')).toBe(reader)
    expect(container.querySelector('.reader-session')?.hasAttribute('hidden')).toBe(true)
    expect(closedDocuments).toEqual([])
    await act(async () => useStore.getState().activateTab('reader:PAPER111'))
    expect(container.querySelector('.reader')).toBe(reader)
    expect(container.querySelector('.reader-session')?.hasAttribute('hidden')).toBe(false)
    expect(startedDocuments).toEqual(['blob:FILE1111'])
    await act(async () => useStore.getState().closeTab('reader:PAPER111'))
    expect(container.querySelector('.reader-session')).toBeNull()
    expect(closedDocuments).toEqual(['blob:FILE1111'])
  })

  it('lists summaries and close readings as notes, not separate tabs', async () => {
    attached.push(
      child('SUMMARY1', 'note', { note: 'Summary content', tags: [{ tag: 'summary' }] }),
      child('READING1', 'note', { note: 'Close reading content', tags: [{ tag: 'close-reading' }] }),
      child('NOTE0001', 'note', { note: 'My own note' }),
    )
    await render()
    const tabs = container.querySelectorAll<HTMLButtonElement>('.reader-notes .rail-tab')
    expect([...tabs].map((tab) => tab.textContent)).toEqual(['Marks', 'Notes (3)'])
    await act(async () => tabs[1]?.click())
    const notes = container.querySelectorAll('.reader-note-body .note-card')
    expect(notes).toHaveLength(3)
    expect([...notes].map((note) => note.textContent).join(' ')).toContain('Summary content')
    expect([...notes].map((note) => note.textContent).join(' ')).toContain('Close reading content')
  })
  it('uses one file switcher, excluding snapshots and supplementary non-PDF files', async () => {
    attached = [
      child('HTML1111', 'attachment', { title: 'Snapshot', contentType: 'text/html' }),
      child('FILE1111', 'attachment', { title: 'paper.pdf', contentType: 'application/pdf' }),
      child('FILE2222', 'attachment', { title: 'supplement.pdf', contentType: 'application/pdf' }),
    ]
    await render()
    const files = container.querySelector<HTMLSelectElement>('.reader-files')!
    expect(files.tagName).toBe('SELECT')
    expect(files.options).toHaveLength(2)
    expect(files.value).toBe('FILE1111')
    expect(documentLoads).not.toContain('blob:HTML1111')
    expect(useStore.getState().tabs).toHaveLength(1)
    await act(async () => {
      files.value = 'FILE2222'
      files.dispatchEvent(new Event('change', { bubbles: true }))
    })
    expect(documentLoads.at(-1)).toBe('blob:FILE2222')
    expect(useStore.getState().tabs).toHaveLength(1)
  })

  it('starts with a document, not three expanded side panels', async () => {
    useStore.setState({ readerLayout: DEFAULT_READER_LAYOUT })
    await render()
    expect(container.querySelector('.reader-notes')).toBeNull()
    expect(container.querySelector('.detail-pane')).toBeNull()
    const toggle = container.querySelector<HTMLButtonElement>('[aria-label="Show or hide notes"]')
    expect(toggle).not.toBeNull()
    await act(async () => toggle?.click())
    expect(container.querySelector('.reader-notes')).not.toBeNull()
  })

  it('uses the inspector shortcut without changing the library layout', async () => {
    useStore.setState({ detailOpen: true, readerLayout: DEFAULT_READER_LAYOUT })
    await render()
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'i' })))
    expect(useStore.getState().readerLayout.detailsOpen).toBe(true)
    expect(useStore.getState().detailOpen).toBe(true)
    expect(container.querySelector('.detail-pane')).not.toBeNull()
  })

  it('can open an attachment directly without looking for a PDF below that attachment', async () => {
    useStore.setState({
      tabs: [{ id: 'reader:FILE1111', kind: 'reader', title: 'PDF', target: 'FILE1111' }],
      activeTab: 'reader:FILE1111',
    })
    await render()
    expect(container.querySelector('.reader-pages')).not.toBeNull()
    expect(container.querySelectorAll('.note-card')).toHaveLength(2)
  })
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
