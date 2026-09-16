import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act, type ComponentProps } from 'react'
import type { PDFDocumentProxy } from 'pdfjs-dist'

import { PageRail } from './PageRail'

const pdf = vi.hoisted(() => ({ getDocument: vi.fn(), thumbnailFor: vi.fn() }))
vi.mock('pdfjs-dist', () => ({ getDocument: pdf.getDocument }))
vi.mock('../lib/thumbnails', () => ({
  THUMB_WIDTHS: [96, 240, 960],
  thumbnailFor: pdf.thumbnailFor,
}))

/**
 * jsdom has no IntersectionObserver, and the rail is built around one: nothing
 * is drawn until a cell comes near the viewport, because every miss is a full
 * pdf.js page render.
 */
class FakeObserver {
  static instances: FakeObserver[] = []
  targets: Element[] = []
  constructor(
    readonly callback: IntersectionObserverCallback,
    readonly options: IntersectionObserverInit,
  ) {
    FakeObserver.instances.push(this)
  }
  observe(el: Element) {
    this.targets.push(el)
  }
  disconnect = vi.fn()
  intersect(pages: number[], isIntersecting: boolean) {
    const entries = this.targets
      .filter((target) => pages.includes(Number((target as HTMLElement).dataset.page)))
      .map((target) => ({ target, isIntersecting }) as IntersectionObserverEntry)
    this.callback(entries, this as unknown as IntersectionObserver)
  }
}

let container: HTMLElement
let root: Root
const renders: Array<{ page: number; canvas: HTMLCanvasElement }> = []

function deferred() {
  let resolve!: () => void
  let reject!: (error: Error) => void
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

function documentWith(
  render = (_page: number) => ({ promise: Promise.resolve(), cancel: vi.fn() }),
  width = 600,
  height = 800,
): PDFDocumentProxy {
  return {
    getPage: vi.fn(async (page: number) => ({
      getViewport: ({ scale }: { scale: number }) => ({ width: width * scale, height: height * scale }),
      render: ({ canvas }: { canvas: HTMLCanvasElement }) => {
        renders.push({ page, canvas })
        return render(page)
      },
    })),
  } as unknown as PDFDocumentProxy
}

beforeEach(() => {
  FakeObserver.instances = []
  renders.length = 0
  pdf.getDocument.mockReset()
  pdf.thumbnailFor.mockReset().mockResolvedValue(null)
  vi.stubGlobal('IntersectionObserver', FakeObserver)
  vi.stubGlobal('devicePixelRatio', 1)
  vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('Unexpected network access')))
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({} as never)
  Element.prototype.scrollIntoView = vi.fn()
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

function mount(current: number, onJump = vi.fn(), props: Partial<ComponentProps<typeof PageRail>> = {}) {
  act(() => {
    root.render(
      <PageRail
        library={1}
        attachmentKey="ABCD1234"
        pages={[1, 2, 3, 4, 5]}
        current={current}
        onJump={onJump}
        {...props}
      />,
    )
  })
  return onJump
}

async function intersect(pages: number[], visible = true, observer = FakeObserver.instances.at(-1)!) {
  await act(async () => observer.intersect(pages, visible))
}

describe('the reader page rail', () => {
  it('offers a cell per page and marks the one being read', () => {
    mount(3)
    expect(container.querySelectorAll('[data-page]')).toHaveLength(5)
    expect(container.querySelector('[data-active="true"]')?.getAttribute('data-page')).toBe('3')
  })

  it('draws nothing until a cell comes near the viewport', () => {
    mount(1)
    // A rail that drew every page would render a whole thesis before showing
    // page one.
    expect(container.querySelectorAll('img')).toHaveLength(0)
    expect(FakeObserver.instances[0]?.targets).toHaveLength(5)
  })

  it('jumps to the page that was clicked', () => {
    const onJump = mount(1)
    act(() => {
      ;(container.querySelector('[data-page="4"]') as HTMLElement).click()
    })
    expect(onJump).toHaveBeenCalledWith(4)
  })

  it('keeps the page being read in view', () => {
    container.scrollTop = 51
    const outerScroll = document.documentElement.scrollTop
    document.documentElement.scrollTop = 73
    const scrollWindow = vi.spyOn(window, 'scrollTo').mockImplementation(() => {})
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
      if (this.classList.contains('page-rail')) return new DOMRect(0, 0, 132, 200)
      const page = (this as HTMLElement).dataset.page
      return new DOMRect(0, page === '3' ? -40 : page === '4' ? 50 : 240, 96, 124)
    })
    mount(2)
    expect(container.querySelector('.page-rail')?.scrollTop).toBe(164)
    mount(3)
    expect(container.querySelector('.page-rail')?.scrollTop).toBe(124)
    mount(4)
    expect(container.querySelector('.page-rail')?.scrollTop).toBe(124)
    // Do not scroll the outer PDF/document when following the reader in the rail.
    expect(Element.prototype.scrollIntoView).not.toHaveBeenCalled()
    expect(scrollWindow).not.toHaveBeenCalled()
    expect(container.scrollTop).toBe(51)
    expect(document.documentElement.scrollTop).toBe(73)
    document.documentElement.scrollTop = outerScroll
  })

  it('shows every page number', () => {
    mount(1)
    const numbers = [...container.querySelectorAll('.page-number')].map((n) => n.textContent)
    expect(numbers).toEqual(['1', '2', '3', '4', '5'])
  })
})

describe('thumbnails sharing the reader document', () => {
  it('uses only doc.getPage for intersecting cells, without loading another PDF or making HTTP requests', async () => {
    const doc = documentWith()
    mount(1, vi.fn(), { doc })
    expect(doc.getPage).not.toHaveBeenCalled()
    expect(FakeObserver.instances[0]?.options.root).toBe(container.querySelector('.page-rail'))
    await intersect([2, 4])
    expect(doc.getPage).toHaveBeenCalledTimes(2)
    expect(doc.getPage).toHaveBeenCalledWith(2)
    expect(doc.getPage).toHaveBeenCalledWith(4)
    const painted = [...container.querySelectorAll('canvas')].map((canvas) =>
      canvas.closest<HTMLElement>('[data-page]')?.dataset.page,
    )
    expect(painted).toEqual(['2', '4'])
    expect(pdf.getDocument).not.toHaveBeenCalled()
    expect(pdf.thumbnailFor).not.toHaveBeenCalled()
    expect(fetch).not.toHaveBeenCalled()
    expect(container.querySelector('img')).toBeNull()
  })

  it('retains canvases only for intersecting cells and releases completed backing stores', async () => {
    mount(1, vi.fn(), { doc: documentWith() })
    await intersect([1, 2])
    const first = container.querySelector<HTMLCanvasElement>('[data-page="1"] canvas')!
    const second = container.querySelector<HTMLCanvasElement>('[data-page="2"] canvas')!
    await intersect([1], false)
    expect(container.querySelectorAll('canvas')).toHaveLength(1)
    expect(container.querySelector('canvas')).toBe(second)
    expect(first.width).toBe(0)
    expect(first.height).toBe(0)
    await intersect([2], false)
    expect(container.querySelector('canvas')).toBeNull()
    expect(second.width).toBe(0)
  })

  it('does not repaint or cancel thumbnails when the current-page indicator rerenders', async () => {
    const cancel = vi.fn()
    const doc = documentWith(() => ({ promise: Promise.resolve(), cancel }))
    mount(1, vi.fn(), { doc })
    await intersect([1])
    const original = container.querySelector('canvas')
    mount(2, vi.fn(), { doc })
    await act(async () => {})
    expect(doc.getPage).toHaveBeenCalledTimes(1)
    expect(renders).toHaveLength(1)
    expect(container.querySelector('canvas')).toBe(original)
    expect(cancel).not.toHaveBeenCalled()
  })

  it('uses independent canvases during rapid visibility toggles and never commits an obsolete render', async () => {
    const pending = deferred()
    const cancel = vi.fn()
    const render = vi.fn()
      .mockReturnValueOnce({ promise: pending.promise, cancel })
      .mockImplementation(() => ({ promise: Promise.resolve(), cancel: vi.fn() }))
    const doc = documentWith(render)
    mount(1, vi.fn(), { doc })
    await intersect([1])
    const obsolete = renders[0]!.canvas
    expect(container.querySelector('canvas')).toBeNull()
    await intersect([1], false)
    expect(cancel).toHaveBeenCalledTimes(1)
    // The task still owns this bitmap until cancellation settles.
    expect(obsolete.width).toBeGreaterThan(0)
    await intersect([1])
    const current = container.querySelector('canvas')!
    expect(current).toBe(renders[1]!.canvas)
    expect(current).not.toBe(obsolete)
    await act(async () => pending.resolve())
    expect(container.querySelector('canvas')).toBe(current)
    expect(container.querySelectorAll('canvas')).toHaveLength(1)
    expect(obsolete.width).toBe(0)
    expect(obsolete.height).toBe(0)
    expect(container.querySelector('[role=alert]')).toBeNull()
  })

  it('observes cancellation rejection after a cell unmounts', async () => {
    const pending = deferred()
    const cancel = vi.fn(() => pending.reject(new Error('Rendering cancelled')))
    mount(1, vi.fn(), { doc: documentWith(() => ({ promise: pending.promise, cancel })) })
    await intersect([1])
    await intersect([1], false)
    expect(cancel).toHaveBeenCalledTimes(1)
    expect(renders[0]!.canvas.width).toBe(0)
    expect(container.querySelector('canvas')).toBeNull()
  })

  it('does not render a page whose lookup completed after it left the viewport', async () => {
    let resolve!: (page: Awaited<ReturnType<PDFDocumentProxy['getPage']>>) => void
    const pending = new Promise<Awaited<ReturnType<PDFDocumentProxy['getPage']>>>((yes) => { resolve = yes })
    const doc = documentWith()
    const page = await doc.getPage(1)
    doc.getPage = vi.fn().mockReturnValue(pending)
    mount(1, vi.fn(), { doc })
    await intersect([1])
    await intersect([1], false)
    await act(async () => resolve(page))
    expect(renders).toHaveLength(0)
    expect(container.querySelector('canvas')).toBeNull()
  })

  it('ignores late visibility callbacks from a disconnected observer', async () => {
    const first = documentWith()
    const second = documentWith()
    mount(1, vi.fn(), { doc: first })
    const observer = FakeObserver.instances[0]!
    mount(1, vi.fn(), { doc: second, attachmentKey: 'NEXT1234' })
    expect(observer.disconnect).toHaveBeenCalledTimes(1)
    await intersect([3], true, observer)
    expect(first.getPage).not.toHaveBeenCalled()
    expect(second.getPage).not.toHaveBeenCalled()
    expect(container.querySelector('canvas')).toBeNull()
    await intersect([4])
    expect(second.getPage).toHaveBeenCalledWith(4)
  })

  it('reports render failures once, releases failed canvases, and recovers on another document', async () => {
    const broken = documentWith(() => { throw new Error('Thumbnail failed') })
    mount(1, vi.fn(), { doc: broken })
    await intersect([1])
    expect(broken.getPage).toHaveBeenCalledTimes(1)
    expect(container.querySelector('[role=alert]')?.getAttribute('title')).toBe('Thumbnail failed')
    expect(renders[0]!.canvas.width).toBe(0)
    const next = documentWith()
    mount(1, vi.fn(), { doc: next })
    await intersect([1])
    expect(container.querySelector('[role=alert]')).toBeNull()
    expect(container.querySelector('canvas')).not.toBeNull()
  })

  it('releases a canvas when a context could not be created', async () => {
    const context = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
    const doc = documentWith()
    mount(1, vi.fn(), { doc })
    await intersect([1])
    const canvas = context.mock.contexts[0] as HTMLCanvasElement
    expect(canvas.width).toBe(0)
    expect(canvas.height).toBe(0)
    expect(doc.getPage).toHaveBeenCalledTimes(1)
    expect(container.querySelector('[role=alert]')?.getAttribute('title'))
      .toBe('PDF thumbnail canvas rendering is unavailable')
  })

  it('bounds the backing store of an extremely tall page on a HiDPI screen', async () => {
    vi.stubGlobal('devicePixelRatio', 4)
    mount(1, vi.fn(), { doc: documentWith(undefined, 600, 1000000) })
    await intersect([1])
    const canvas = container.querySelector('canvas')!
    expect(canvas.width).toBeGreaterThan(0)
    expect(canvas.width).toBeLessThanOrEqual(192)
    expect(canvas.height).toBeLessThanOrEqual(256)
    expect(parseFloat(canvas.style.height)).toBeLessThanOrEqual(128)
    expect(parseFloat(canvas.style.width) / parseFloat(canvas.style.height)).toBeCloseTo(600 / 1000000)
  })

  it('retains the cached thumbnail fallback when no document is supplied', async () => {
    mount(1)
    await intersect([2])
    expect(pdf.thumbnailFor).toHaveBeenCalledWith(1, 'ABCD1234', 2, 96)
    expect(pdf.getDocument).not.toHaveBeenCalled()
    expect(fetch).not.toHaveBeenCalled()
  })
})
