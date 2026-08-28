import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { PdfPage } from './PdfPage'
import type { PDFDocumentProxy } from 'pdfjs-dist'

// The component imports pdf.js dynamically to build its text layer. Loading
// the real one here drags in canvas code that jsdom cannot run (no DOMMatrix),
// which surfaces as unhandled rejections — noise that would hide a real one.
vi.mock('pdfjs-dist', () => ({
  TextLayer: class {
    render() {
      return Promise.resolve()
    }
  },
}))

const rendered: number[] = []

/** Just enough of a pdf.js document to see what was asked of it. */
function doc(): PDFDocumentProxy {
  return {
    getPage: vi.fn(async (n: number) => ({
      getViewport: ({ scale }: { scale: number }) => ({ width: 600 * scale, height: 800 * scale }),
      render: (opts: { canvas: HTMLCanvasElement }) => {
        rendered.push(n)
        // jsdom canvases have no 2d context worth the name; only the fact of
        // the call matters here.
        void opts
        return { promise: Promise.resolve(), cancel: vi.fn() }
      },
      getTextContent: vi.fn(async () => ({ items: [] })),
    })),
  } as unknown as PDFDocumentProxy
}

let container: HTMLElement
let root: Root

beforeEach(() => {
  rendered.length = 0
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  HTMLCanvasElement.prototype.getContext = vi.fn(() => ({})) as never
})
afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

function mount(detail: 'full' | 'text' | 'none') {
  act(() => {
    root.render(
      <PdfPage
        doc={doc()}
        pageNumber={7}
        zoom={1}
        annotations={[]}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
        detail={detail}
        reserve={{ width: 600, height: 800 }}
      />,
    )
  })
}

describe('a page that is not on screen', () => {
  it('still holds its place', () => {
    // The scrollbar has to mean something, and a page that collapsed when it
    // scrolled away would drag everything below it upwards.
    mount('none')
    const page = container.querySelector<HTMLElement>('.pdf-page')
    expect(page?.style.width).toBe('600px')
    expect(page?.style.height).toBe('800px')
    expect(page?.dataset.page).toBe('7')
  })

  it('has neither a canvas nor a text layer', () => {
    mount('none')
    expect(container.querySelector('canvas')).toBeNull()
    expect(container.querySelector('.pdf-text')).toBeNull()
  })

  it('keeps its text layer while a search is running', () => {
    // Find works over the rendered spans, so a page with no text layer is a
    // page the search silently skips — the regression virtualising invites.
    mount('text')
    expect(container.querySelector('.pdf-text')).not.toBeNull()
    // But not the canvas, which is what costs megabytes.
    expect(container.querySelector('canvas')).toBeNull()
  })

  it('draws only when it is near', async () => {
    mount('text')
    await act(async () => {})
    expect(rendered).toEqual([])

    mount('full')
    await act(async () => {})
    expect(rendered).toEqual([7])
  })
})
