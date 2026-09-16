import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'
import { readFileSync } from 'node:fs'

import { PdfPage, type PdfPageProps } from './PdfPage'
import { MAX_CANVAS_DIMENSION, MAX_CANVAS_PIXELS } from './pdfRendering'
import type { PDFDocumentProxy } from 'pdfjs-dist'
import type { Annotation } from '../lib/annotations'

const textLayer = vi.hoisted(() => ({ render: vi.fn(), cancel: vi.fn() }))

// Match PDF.js 6's custom-property text runs without a native canvas in jsdom.
vi.mock('pdfjs-dist', () => ({
  TextLayer: class {
    container: HTMLDivElement
    constructor({ container, viewport }: { container: HTMLDivElement; viewport: { rotation: number } }) {
      this.container = container
      container.dataset.mainRotation = String(viewport.rotation)
    }
    render() {
      return textLayer.render(this.container)
    }
    cancel() {
      textLayer.cancel()
    }
  },
}))

const rendered: number[] = []
const outputs: RenderOptions[] = []
interface RenderOptions {
  canvas: HTMLCanvasElement
  viewport: { width: number; height: number; scale: number }
  transform: number[]
}

function deferred() {
  let resolve!: () => void
  let reject!: (error: Error) => void
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

/** Just enough of a pdf.js document to see what was asked of it. */
function doc({
  width = 600,
  height = 800,
  rotation = 0,
  userUnit = 1,
  render = () => ({ promise: Promise.resolve(), cancel: vi.fn() }),
}: {
  width?: number
  height?: number
  rotation?: number
  userUnit?: number
  render?: (options: RenderOptions) => { promise: Promise<void>; cancel: () => void }
} = {}): PDFDocumentProxy {
  return {
    getPage: vi.fn(async (n: number) => ({
      getViewport: ({ scale }: { scale: number }) => ({
        width: (rotation % 180 ? height : width) * scale * userUnit,
        height: (rotation % 180 ? width : height) * scale * userUnit,
        scale,
        rotation,
        userUnit,
      }),
      render: (opts: RenderOptions) => {
        rendered.push(n)
        outputs.push(opts)
        return render(opts)
      },
      getTextContent: vi.fn(async () => ({ items: [] })),
    })),
  } as unknown as PDFDocumentProxy
}

let container: HTMLElement
let root: Root
let source: PDFDocumentProxy
let stylesheet: HTMLStyleElement

beforeEach(() => {
  rendered.length = 0
  outputs.length = 0
  source = doc()
  textLayer.render.mockReset().mockImplementation((layer: HTMLElement) => {
    const span = document.createElement('span')
    span.style.setProperty('--font-height', '12px')
    span.style.setProperty('--scale-x', '0.95')
    span.textContent = 'Selectable words'
    layer.append(span)
    return Promise.resolve()
  })
  textLayer.cancel.mockReset()
  vi.stubGlobal('devicePixelRatio', 1)
  container = document.createElement('div')
  document.body.append(container)
  stylesheet = document.createElement('style')
  stylesheet.textContent = readFileSync('src/pages/pdf-page.css', 'utf8')
  document.head.append(stylesheet)
  root = createRoot(container)
  HTMLCanvasElement.prototype.getContext = vi.fn(() => ({})) as never
})
afterEach(() => {
  act(() => root.unmount())
  container.remove()
  stylesheet.remove()
  window.getSelection()?.removeAllRanges()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

function mount(detail: 'full' | 'text' | 'none', props: Partial<PdfPageProps> = {}) {
  act(() => {
    root.render(
      <PdfPage
        doc={source}
        pageNumber={7}
        zoom={1}
        annotations={[]}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
        detail={detail}
        reserve={{ width: 600, height: 800 }}
        {...props}
      />,
    )
  })
}

async function paint(detail: 'full' | 'text' | 'none', props: Partial<PdfPageProps> = {}) {
  mount(detail, props)
  await act(async () => {})
}

describe('clicking existing annotations', () => {
  const mark: Annotation = {
    key: 'MARK0001', version: 3, page: 7, kind: 'highlight', space: 'fraction',
    text: 'Marked words', comment: '', colour: 'amber',
    rects: [{ x: 0.1, y: 0.2, w: 0.3, h: 0.04 }],
  }

  it('hit-tests through selectable text instead of intercepting its pointer events', async () => {
    const selected = vi.fn()
    await paint('full', { annotations: [mark], onAnnotationSelect: selected })
    const page = container.querySelector<HTMLElement>('.pdf-page')!
    vi.spyOn(page, 'getBoundingClientRect').mockReturnValue(new DOMRect(100, 50, 600, 800))
    const text = page.querySelector('.pdf-text-layer span')!
    act(() => {
      text.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0, clientX: 190, clientY: 218 }))
      text.dispatchEvent(new MouseEvent('click', { bubbles: true, button: 0, detail: 1, clientX: 190, clientY: 218 }))
    })
    expect(selected).toHaveBeenCalledWith(mark, [{ left: 160, top: 210, width: 180, height: 32 }])
    expect(getComputedStyle(page.querySelector('.pdf-highlight')!).pointerEvents).toBe('none')
  })

  it('does not open editing tools after dragging across a mark', async () => {
    const selected = vi.fn()
    await paint('full', { annotations: [mark], onAnnotationSelect: selected })
    const page = container.querySelector<HTMLElement>('.pdf-page')!
    vi.spyOn(page, 'getBoundingClientRect').mockReturnValue(new DOMRect(100, 50, 600, 800))
    act(() => {
      page.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, clientX: 170, clientY: 218 }))
      page.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1, clientX: 240, clientY: 218 }))
    })
    expect(selected).not.toHaveBeenCalled()
  })
})

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

  it('keeps its text layer while a search is running', async () => {
    // Find works over the rendered spans, so a page with no text layer is a
    // page the search silently skips — the regression virtualising invites.
    await paint('text')
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

  it('resizes a previously measured offscreen page without rendering it again', async () => {
    source = doc({ width: 321, height: 987 })
    await paint('full', { zoom: 1.25 })
    await paint('none', { zoom: 2.5, reserve: { width: 1500, height: 2000 } })
    const page = container.querySelector<HTMLElement>('.pdf-page')
    expect(page?.style.width).toBe('802.5px')
    expect(page?.style.height).toBe('2467.5px')
    expect(source.getPage).toHaveBeenCalledTimes(1)
    expect(rendered).toEqual([7])
    expect(container.querySelector('canvas')).toBeNull()
  })

  it('releases its bitmap, but preserves search text when leaving the viewport', async () => {
    await paint('full')
    const canvas = container.querySelector('canvas')!
    const span = container.querySelector('.pdf-text span')
    await paint('text', { zoom: 2 })
    expect(canvas.width).toBe(0)
    expect(canvas.height).toBe(0)
    expect(container.querySelector('.pdf-text span')).toBe(span)
    expect(textLayer.render).toHaveBeenCalledTimes(1)
  })
})

describe('zoom rendering', () => {
  it('updates layout immediately without replacing text, selection, or the settled bitmap', async () => {
    await paint('full', { zoom: 1, renderZoom: 1 })
    const canvas = container.querySelector('canvas')
    const text = container.querySelector('.pdf-text span')!.firstChild!
    const range = document.createRange()
    range.selectNodeContents(text)
    window.getSelection()?.addRange(range)

    for (const zoom of [1.1, 1.3, 1.75]) {
      await paint('full', { zoom, renderZoom: 1 })
    }
    expect(container.querySelector<HTMLElement>('.pdf-page')?.style.width).toBe('1050px')
    expect(container.querySelector<HTMLElement>('.pdf-text')?.style.transform).toBe('scale(1.75)')
    expect(container.querySelector('canvas')).toBe(canvas)
    expect(window.getSelection()?.anchorNode).toBe(text)
    expect(window.getSelection()?.toString()).toBe('Selectable words')
    expect(rendered).toEqual([7])
    expect(textLayer.render).toHaveBeenCalledTimes(1)
  })

  it('keeps the last bitmap and text visible until the replacement is ready', async () => {
    const pending = deferred()
    const render = vi.fn()
      .mockReturnValueOnce({ promise: Promise.resolve(), cancel: vi.fn() })
      .mockReturnValueOnce({ promise: pending.promise, cancel: vi.fn() })
    source = doc({ render })
    await paint('full')
    const original = container.querySelector('canvas')!
    const span = container.querySelector('.pdf-text span')
    await paint('full', { zoom: 2 })
    expect(container.querySelector('canvas')).toBe(original)
    expect(original.width).toBe(600)
    expect(container.querySelector('.pdf-text span')).toBe(span)

    await act(async () => { pending.resolve() })
    expect(container.querySelector('canvas')).not.toBe(original)
    expect(container.querySelector('canvas')?.width).toBe(1200)
    expect(original.width).toBe(0)
    expect(container.querySelector('.pdf-text span')).toBe(span)
  })

  it('awaits stalled cancellation and skips intermediate zoom requests', async () => {
    const pending = deferred()
    const cancel = vi.fn()
    const render = vi.fn()
      .mockReturnValueOnce({ promise: Promise.resolve(), cancel: vi.fn() })
      .mockReturnValueOnce({ promise: pending.promise, cancel })
      .mockImplementation(() => ({ promise: Promise.resolve(), cancel: vi.fn() }))
    source = doc({ render })
    await paint('full')
    const original = container.querySelector('canvas')
    await paint('full', { zoom: 2 })
    await paint('full', { zoom: 2.5 })
    await paint('full', { zoom: 3 })
    expect(cancel).toHaveBeenCalledTimes(1)
    expect(render).toHaveBeenCalledTimes(2)
    expect(container.querySelector('canvas')).toBe(original)

    await act(async () => {
      pending.reject(Object.assign(new Error('Cancelled'), { name: 'RenderingCancelledException' }))
    })
    expect(render).toHaveBeenCalledTimes(3)
    expect(outputs.map((output) => output.viewport.scale)).toEqual([1, 2, 3])
    expect(container.querySelector('canvas')?.width).toBe(1800)
    expect(outputs[1]?.canvas.width).toBe(0)
    expect(container.querySelector('[role=alert]')).toBeNull()
  })

  it('handles cancellation while unmounting a stalled canvas and text task', async () => {
    const raster = deferred()
    const text = deferred()
    const cancel = vi.fn(() => raster.reject(new Error('Cancelled')))
    source = doc({ render: () => ({ promise: raster.promise, cancel }) })
    textLayer.render.mockImplementation(() => text.promise)
    textLayer.cancel.mockImplementation(() => text.reject(new Error('Text cancelled')))
    await paint('full')
    await act(async () => root.render(null))
    expect(cancel).toHaveBeenCalledTimes(1)
    expect(textLayer.cancel).toHaveBeenCalledTimes(1)
    expect(outputs[0]?.canvas.width).toBe(0)
  })

  it('uses bounded HiDPI output without shrinking the page layout', async () => {
    vi.stubGlobal('devicePixelRatio', 4)
    source = doc({ width: 9000, height: 12000 })
    await paint('full', { zoom: 8 })
    const output = outputs[0]!
    expect(output.canvas.width * output.canvas.height).toBeLessThanOrEqual(MAX_CANVAS_PIXELS)
    expect(Math.max(output.canvas.width, output.canvas.height)).toBeLessThanOrEqual(MAX_CANVAS_DIMENSION)
    expect(output.transform[0]).toBe(output.canvas.width / output.viewport.width)
    expect(output.transform[3]).toBe(output.canvas.height / output.viewport.height)
    expect(container.querySelector<HTMLElement>('.pdf-page')?.style.width).toBe('72000px')
    expect(container.querySelector<HTMLElement>('.pdf-page')?.style.height).toBe('96000px')
  })

  it('uses device resolution and preserves exact fractional layout dimensions', async () => {
    vi.stubGlobal('devicePixelRatio', 2)
    source = doc({ width: 600.25, height: 800.75 })
    await paint('full', { zoom: 1.2 })
    expect(container.querySelector('canvas')?.width).toBe(Math.floor(600.25 * 1.2 * 2))
    expect(container.querySelector('canvas')?.height).toBe(Math.floor(800.75 * 1.2 * 2))
    const page = container.querySelector<HTMLElement>('.pdf-page')!
    expect(parseFloat(page.style.width)).toBeCloseTo(600.25 * 1.2)
    expect(parseFloat(page.style.height)).toBeCloseTo(800.75 * 1.2)
  })

  it('provides PDF.js 6 text scale and unrotated dimensions for rotated user-unit pages', async () => {
    source = doc({ width: 600, height: 800, rotation: 90, userUnit: 2 })
    await paint('text', { zoom: 1.5 })
    const layer = container.querySelector<HTMLElement>('.pdf-text-layer')!
    expect(layer.style.getPropertyValue('--total-scale-factor')).toBe('2')
    expect(layer.dataset.mainRotation).toBe('90')
    expect(layer.style.width).toBe('1200px')
    expect(layer.style.height).toBe('1600px')
    expect(container.querySelector<HTMLElement>('.pdf-page')?.style.width).toBe('2400px')
    expect(container.querySelector<HTMLElement>('.pdf-page')?.style.height).toBe('1800px')
  })

  it('styles PDF.js custom-property spans while leaving nested find marks inline', async () => {
    await paint('text')
    const span = container.querySelector<HTMLElement>('.pdf-text span')!
    const found = document.createElement('span')
    found.dataset.found = 'true'
    found.textContent = 'Selectable'
    span.replaceChildren(found)
    // jsdom cannot resolve arithmetic on inherited CSS custom properties.
    const rule = Array.from(stylesheet.sheet!.cssRules).find((entry) =>
      entry instanceof CSSStyleRule && entry.selectorText.includes('> :not(.markedContent)'),
    ) as CSSStyleRule
    expect(rule.style.getPropertyValue('font-size')).toContain('--font-height')
    expect(rule.style.transform).toContain('--scale-x')
    expect(getComputedStyle(found).position).toBe('static')
    expect(getComputedStyle(found).transform).toBe('none')
    expect(getComputedStyle(container.querySelector('.pdf-text')!).opacity).toBe('1')
  })
})

describe('rendering failures and safe selection', () => {
  it('reports a page loading failure', async () => {
    source.getPage = vi.fn().mockRejectedValue(new Error('Page missing'))
    await paint('full')
    expect(container.querySelector('[role=alert]')?.getAttribute('title')).toBe('Page missing')
  })

  it('reports a raster failure while retaining the previous bitmap', async () => {
    const render = vi.fn()
      .mockImplementationOnce(() => ({ promise: Promise.resolve(), cancel: vi.fn() }))
      .mockImplementationOnce(() => ({ promise: Promise.reject(new Error('Paint failed')), cancel: vi.fn() }))
    source = doc({ render })
    await paint('full')
    const original = container.querySelector('canvas')
    await paint('full', { zoom: 2 })
    expect(container.querySelector('[role=alert]')?.getAttribute('title')).toBe('Paint failed')
    expect(container.querySelector('canvas')).toBe(original)
  })

  it('reports a missing canvas context rather than leaving a blank page silently', async () => {
    HTMLCanvasElement.prototype.getContext = vi.fn(() => null) as never
    await paint('full')
    expect(container.querySelector('[role=alert]')?.getAttribute('title')).toBe('PDF canvas rendering is unavailable')
  })

  it('reports a text layer failure', async () => {
    textLayer.render.mockRejectedValue(new Error('Text failed'))
    await paint('text')
    expect(container.querySelector('[role=alert]')?.getAttribute('title')).toBe('Text failed')
  })

  it('reports a text extraction failure without an unhandled rejection', async () => {
    const page = await source.getPage(7)
    page.getTextContent = vi.fn().mockRejectedValue(new Error('Extraction failed'))
    source.getPage = vi.fn().mockResolvedValue(page)
    await paint('text')
    expect(container.querySelector('[role=alert]')?.getAttribute('title')).toBe('Extraction failed')
  })

  it('reports selection without deleting a highlight on double click', async () => {
    const onSelect = vi.fn()
    const onRemove = vi.fn()
    await paint('full', {
      onSelect,
      onRemove,
      annotations: [{
        key: 'mark', page: 7, kind: 'highlight', space: 'fraction',
        rects: [{ x: 0.1, y: 0.1, w: 0.2, h: 0.1 }],
        colour: 'amber', text: 'Selected', comment: '',
      }],
    })
    const highlight = container.querySelector('.pdf-highlight')!
    act(() => {
      highlight.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
      container.querySelector('.pdf-text span')!.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))
    })
    expect(onRemove).not.toHaveBeenCalled()
    expect(getComputedStyle(highlight).pointerEvents).toBe('none')
    expect(onSelect).toHaveBeenCalledWith(7, expect.objectContaining({ width: 0, height: 0 }))
  })
})
