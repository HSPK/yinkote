import { act, useRef } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { MAX_ZOOM, MIN_ZOOM, clampZoom, useReaderZoom, wheelZoom } from './useReaderZoom'

let container: HTMLDivElement
let root: Root

function Harness() {
  const scroller = useRef<HTMLDivElement>(null)
  const { zoom, renderZoom, zoomAt, step } = useReaderZoom(scroller, 'PDF')
  return <>
    <div ref={scroller} className="viewport" data-zoom={zoom} data-raster={renderZoom}>
      <div className="pdf-page" style={{ width: 600 * zoom, height: 800 * zoom }} />
    </div>
    <button onClick={() => step(1)}>+</button>
    <button onClick={() => zoomAt(1)}>100%</button>
  </>
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('requestAnimationFrame', (run: FrameRequestCallback) =>
    window.setTimeout(() => run(performance.now()), 16))
  vi.stubGlobal('cancelAnimationFrame', (id: number) => window.clearTimeout(id))
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    if (this.classList.contains('viewport')) return new DOMRect(10, 20, 800, 600)
    const parent = this.parentElement!
    return new DOMRect(10 - parent.scrollLeft, 20 - parent.scrollTop,
      parseFloat(this.style.width), parseFloat(this.style.height))
  })
  act(() => root.render(<Harness />))
  const viewport = container.querySelector<HTMLElement>('.viewport')!
  Object.defineProperties(viewport, {
    clientHeight: { value: 600 },
    clientWidth: { value: 800 },
  })
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

function wheel(options: WheelEventInit) {
  const event = new WheelEvent('wheel', {
    bubbles: true, cancelable: true, clientX: 310, clientY: 420, ...options,
  })
  container.querySelector('.viewport')!.dispatchEvent(event)
  return event
}

describe('reader zoom gestures', () => {
  it('leaves ordinary trackpad scrolling to the browser', () => {
    expect(wheel({ deltaY: 60, deltaX: 30 }).defaultPrevented).toBe(false)
    act(() => vi.advanceTimersByTime(160))
    expect(container.querySelector<HTMLElement>('.viewport')?.dataset.zoom).toBe('1.2')
  })

  it('anchors Ctrl+wheel and defers expensive painting until the gesture settles', () => {
    expect(wheel({ deltaY: -100, ctrlKey: true }).defaultPrevented).toBe(true)
    const viewport = container.querySelector<HTMLElement>('.viewport')!
    expect(viewport.dataset.zoom).toBe('1.2')
    act(() => vi.advanceTimersByTime(16))
    const zoom = Number(viewport.dataset.zoom)
    expect(zoom).toBeGreaterThan(1.2)
    expect(viewport.scrollTop).toBeCloseTo(400 * (zoom / 1.2 - 1))
    expect(viewport.scrollLeft).toBeCloseTo(300 * (zoom / 1.2 - 1))
    expect(viewport.dataset.raster).toBe('1.2')
    act(() => vi.advanceTimersByTime(140))
    expect(Number(viewport.dataset.raster)).toBe(zoom)
  })

  it('accumulates trackpad pinch deltas without painting each wheel event', () => {
    wheel({ deltaY: -10, ctrlKey: true })
    wheel({ deltaY: -10, ctrlKey: true })
    act(() => vi.advanceTimersByTime(16))
    expect(Number(container.querySelector<HTMLElement>('.viewport')?.dataset.zoom))
      .toBeCloseTo(wheelZoom(1.2, -20, 0, 600))
  })

  it('normalises wheel units and bounds pathological scales', () => {
    expect(wheelZoom(1, 1, 1, 600)).toBe(wheelZoom(1, 16, 0, 600))
    expect(wheelZoom(1, 0.1, 2, 600)).toBe(wheelZoom(1, 60, 0, 600))
    expect(clampZoom(0)).toBe(MIN_ZOOM)
    expect(clampZoom(100)).toBe(MAX_ZOOM)
    expect(clampZoom(NaN)).toBe(1.2)
  })

  it('handles Safari pinch events without zooming the workbench', () => {
    const viewport = container.querySelector<HTMLElement>('.viewport')!
    const start = new Event('gesturestart', { cancelable: true })
    viewport.dispatchEvent(start)
    expect(start.defaultPrevented).toBe(true)
    const change = new Event('gesturechange', { cancelable: true })
    Object.assign(change, { scale: 1.5, clientX: 310, clientY: 420 })
    viewport.dispatchEvent(change)
    expect(change.defaultPrevented).toBe(true)
    act(() => vi.advanceTimersByTime(16))
    expect(Number(viewport.dataset.zoom)).toBeCloseTo(1.8)
    viewport.dispatchEvent(new Event('gestureend'))
  })

  it('removes gesture handlers when the reader closes', () => {
    const viewport = container.querySelector<HTMLElement>('.viewport')!
    act(() => root.render(null))
    const event = new WheelEvent('wheel', { ctrlKey: true, cancelable: true })
    viewport.dispatchEvent(event)
    expect(event.defaultPrevented).toBe(false)
  })
})
