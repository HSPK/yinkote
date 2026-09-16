import { act, createRef } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PdfSelectionOverlay } from './PdfSelectionOverlay'
import { pdfSelection } from '../lib/pdf-selection'

vi.mock('../lib/pdf-selection', () => ({ pdfSelection: vi.fn() }))
let container: HTMLElement
let root: Root
const scroller = createRef<HTMLDivElement>()

function render(active = true, zoom = 1) {
  act(() => root.render(
    <div ref={scroller}>
      <div className="pdf-page" data-page="1" />
      <PdfSelectionOverlay root={scroller} active={active} ready="doc" zoom={zoom} />
    </div>,
  ))
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) =>
    window.setTimeout(() => callback(0), 16))
  vi.stubGlobal('cancelAnimationFrame', (id: number) => window.clearTimeout(id))
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(new DOMRect(50, 100, 600, 800))
  vi.mocked(pdfSelection).mockReturnValue([
    { page: 1, text: 'Words with spaces', rects: [{ x: 0.1, y: 0.2, w: 0.5, h: 0.02 }] },
  ])
  vi.mocked(pdfSelection).mockClear()
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('live PDF selection painting', () => {
  it('coalesces selection events into one frame without rewriting native text', () => {
    render()
    for (let i = 0; i < 30; i++) document.dispatchEvent(new Event('selectionchange'))
    expect(pdfSelection).not.toHaveBeenCalled()
    act(() => vi.advanceTimersByTime(16))
    expect(pdfSelection).toHaveBeenCalledTimes(1)
    expect(scroller.current?.dataset.continuousSelection).toBe('true')
    const line = container.querySelector<HTMLElement>('.pdf-live-selection-line')
    expect(line?.style.width).toBe('300px')
    document.dispatchEvent(new Event('selectionchange'))
    act(() => vi.advanceTimersByTime(16))
    expect(container.querySelector('.pdf-live-selection-line')).toBe(line)
  })

  it('removes custom paint and restores native selection when the range is cleared', () => {
    render()
    act(() => vi.advanceTimersByTime(16))
    vi.mocked(pdfSelection).mockReturnValue([])
    document.dispatchEvent(new Event('selectionchange'))
    act(() => vi.advanceTimersByTime(16))
    expect(container.querySelector('.pdf-live-selection')).toBeNull()
    expect(scroller.current?.hasAttribute('data-continuous-selection')).toBe(false)
  })

  it('stops painting when a PDF becomes inactive', () => {
    render()
    act(() => vi.advanceTimersByTime(16))
    render(false)
    expect(container.querySelector('.pdf-live-selection')).toBeNull()
    expect(scroller.current?.hasAttribute('data-continuous-selection')).toBe(false)
  })
})
