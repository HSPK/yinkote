import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { PDFDocumentProxy } from 'pdfjs-dist'

import { ReaderView } from './ReaderView'
import { useStore } from '../state/store'
import { DEFAULT_READER_LAYOUT } from '../lib/reader-layout'
import { api } from '../api/client'
import { pdfSelection } from '../lib/pdf-selection'
import type { Item } from '../api/types'

vi.mock('../api/client', () => ({
  api: {
    items: { get: vi.fn(), children: vi.fn(), create: vi.fn() },
    readerState: { get: vi.fn(), put: vi.fn(async () => ({})) },
    files: { url: (_lib: number, key: string) => `file:${key}` },
    settings: { put: vi.fn(async () => ({})) },
  },
  connectEvents: () => () => {},
}))
vi.mock('../lib/pdf-selection', () => ({ pdfSelection: vi.fn() }))
vi.mock('./usePdf', () => ({
  usePdf: () => ({ doc, pages: [1, 2], error: null }),
}))
vi.mock('./PdfPage', () => ({
  PdfPage: ({ pageNumber }: { pageNumber: number }) =>
    <div className="pdf-page" data-page={pageNumber}><span className="pdf-text">Text</span></div>,
}))

const doc = {
  getPage: async () => ({ getViewport: () => ({ width: 600, height: 800 }) }),
  getOutline: async () => [],
} as unknown as PDFDocumentProxy

function item(key: string, itemType: string, fields: Partial<Item> = {}): Item {
  return {
    key, itemType, libraryId: 1, version: 1, deleted: false,
    creators: [], tags: [], collections: [], dateAdded: 0, dateModified: 0, ...fields,
  }
}

let container: HTMLDivElement
let root: Root

beforeEach(() => {
  vi.clearAllMocks()
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  vi.stubGlobal('IntersectionObserver', class { observe() {} disconnect() {} })
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) =>
    window.setTimeout(() => callback(0), 16))
  vi.stubGlobal('cancelAnimationFrame', (id: number) => window.clearTimeout(id))
  useStore.setState({
    library: 1,
    readerLayout: { ...DEFAULT_READER_LAYOUT, navigation: false },
  })
  vi.mocked(api.items.get).mockImplementation(async (_lib, key) => item(key, 'journalArticle'))
  vi.mocked(api.items.children).mockImplementation(async (_lib, key) =>
    key.startsWith('PAPER') ? [
      item('FILE1111', 'attachment', { parentKey: key, contentType: 'application/pdf' }),
    ] : [])
  vi.mocked(api.readerState.get).mockResolvedValue({
    zoom: 1.2, lastPage: 1, scrollMode: 'continuous', sidebar: false,
  })
  vi.mocked(pdfSelection).mockReturnValue([
    { page: 1, text: 'End of first page', rects: [{ x: 0.1, y: 0.9, w: 0.5, h: 0.02 }] },
    { page: 2, text: 'Start of next page', rects: [{ x: 0.1, y: 0.1, w: 0.5, h: 0.02 }] },
  ])
  vi.spyOn(window, 'getSelection').mockReturnValue({
    rangeCount: 1,
    getRangeAt: () => ({ getClientRects: () => [new DOMRect(100, 100, 80, 20)] }),
    removeAllRanges: vi.fn(),
  } as unknown as Selection)
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

async function select() {
  await act(async () => root.render(<ReaderView target="PAPER111" />))
  await act(async () => {
    container.querySelector('.reader-pages')!.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))
  })
  expect(container.querySelector('.selection-popup')).not.toBeNull()
}

describe('saving reader annotations', () => {
  it('resizes the navigation pane and persists the committed width', async () => {
    useStore.setState({ readerLayout: { ...DEFAULT_READER_LAYOUT, navigation: true } })
    await act(async () => root.render(<ReaderView target="PAPER111" />))
    const separator = container.querySelector<HTMLElement>('.reader-body [role="separator"]')
    expect(separator).not.toBeNull()
    await act(async () => separator?.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'ArrowRight', bubbles: true,
    })))
    expect(container.querySelector<HTMLElement>('.reader-rail')?.style.width).toBe('208px')
    expect(api.settings.put).toHaveBeenCalledWith({
      readerLayout: JSON.stringify({ ...DEFAULT_READER_LAYOUT, rail: 208 }),
    })
  })

  it('saves one annotation per selected page in a single request', async () => {
    vi.mocked(api.items.create).mockImplementation(async (_lib, drafts) => ({
      created: drafts.map((draft, index) =>
        item(`ANNOT00${index}`, 'annotation', draft as Partial<Item>)),
      failed: [],
    }))
    await select()
    await act(async () => container.querySelector<HTMLButtonElement>('.selection-popup .swatch')?.click())
    expect(api.items.create).toHaveBeenCalledTimes(1)
    const [lib, drafts] = vi.mocked(api.items.create).mock.calls[0]!
    expect(lib).toBe(1)
    expect(drafts).toMatchObject([
      { parentKey: 'FILE1111', annotationPage: '1', annotationText: 'End of first page' },
      { parentKey: 'FILE1111', annotationPage: '2', annotationText: 'Start of next page' },
    ])
    expect(container.querySelector('.selection-popup')).toBeNull()
  })

  it('retries only the failed page after partial batch success', async () => {
    vi.mocked(api.items.create)
      .mockResolvedValueOnce({
        created: [item('ANNOT001', 'annotation', { annotationPage: '1' })],
        failed: [{ index: 1, message: 'busy' }],
      })
      .mockImplementationOnce(async (_lib, drafts) => ({
        created: [item('ANNOT002', 'annotation', drafts[0] as Partial<Item>)], failed: [],
      }))
    await select()
    await act(async () => container.querySelector<HTMLButtonElement>('.selection-popup .swatch')?.click())
    expect(container.querySelector('.selection-popup')).not.toBeNull()
    await act(async () => container.querySelector<HTMLButtonElement>('.selection-popup .swatch')?.click())
    expect(vi.mocked(api.items.create).mock.calls[1]?.[1])
      .toMatchObject([{ annotationPage: '2', parentKey: 'FILE1111' }])
    expect(vi.mocked(api.items.create).mock.calls[1]?.[1]).toHaveLength(1)
  })
})
