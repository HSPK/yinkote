import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react'

import { useDebounced } from '../lib/useDebounced'

export const MIN_ZOOM = 0.25
export const MAX_ZOOM = 8

export function clampZoom(value: number): number {
  return Number.isFinite(value) ? Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, value)) : 1.2
}

export function wheelZoom(zoom: number, delta: number, mode: number, height: number): number {
  const pixels = delta * (mode === 1 ? 16 : mode === 2 ? height : 1)
  return clampZoom(zoom * Math.exp(-Math.max(-300, Math.min(300, pixels)) * 0.002))
}

interface Anchor {
  page: HTMLElement
  x: number
  y: number
  left: number
  top: number
}

/** Layout follows the gesture at most once per frame; expensive PDF painting waits
 *  for it to settle. Keep the point under the pointer fixed, not the page's top. */
export function useReaderZoom(scroller: RefObject<HTMLDivElement>, documentKey: string | null) {
  const [zoom, setScale] = useState(1.2)
  const next = useRef(zoom)
  const frame = useRef(0)
  const anchor = useRef<Anchor | null>(null)
  const renderZoom = useDebounced(zoom, 140)

  const setZoom = useCallback((value: number) => {
    cancelAnimationFrame(frame.current)
    frame.current = 0
    anchor.current = null
    next.current = clampZoom(value)
    setScale(next.current)
  }, [])

  const zoomAt = useCallback((value: number, point?: { x: number; y: number }) => {
    const root = scroller.current
    const scale = clampZoom(value)
    if (!root || scale === next.current) return
    const viewport = root.getBoundingClientRect()
    const x = point?.x ?? viewport.left + root.clientWidth / 2
    const y = point?.y ?? viewport.top + root.clientHeight / 2
    // Several wheel events can precede a render. All of them refer to the same
    // currently painted coordinates; preserve that anchor until the frame.
    if (!frame.current) {
      let closest: HTMLElement | null = null
      let distance = Infinity
      for (const page of root.querySelectorAll<HTMLElement>('.pdf-page')) {
        const box = page.getBoundingClientRect()
        const gap = Math.max(box.top - y, y - box.bottom, 0)
        if (gap < distance) {
          closest = page
          distance = gap
        }
        if (!gap) break
      }
      if (closest) {
        const box = closest.getBoundingClientRect()
        anchor.current = {
          page: closest,
          x: (x - box.left) / box.width,
          y: (y - box.top) / box.height,
          left: x - viewport.left,
          top: y - viewport.top,
        }
      }
      frame.current = requestAnimationFrame(() => {
        frame.current = 0
        setScale(next.current)
      })
    }
    next.current = scale
  }, [scroller])

  useLayoutEffect(() => {
    const root = scroller.current
    const at = anchor.current
    anchor.current = null
    if (!root || !at || !root.contains(at.page)) return
    const box = at.page.getBoundingClientRect()
    const viewport = root.getBoundingClientRect()
    root.scrollLeft += box.left + box.width * at.x - viewport.left - at.left
    root.scrollTop += box.top + box.height * at.y - viewport.top - at.top
  }, [zoom, scroller])

  useEffect(() => {
    const root = scroller.current
    if (!root) return
    const wheel = (event: WheelEvent) => {
      // Trackpad pinch is reported as Ctrl+wheel by Chromium and Firefox.
      // Unmodified two-finger scrolling must stay native, including momentum.
      if (!event.ctrlKey && !event.metaKey) return
      if (!root.querySelector('.pdf-page')) return
      event.preventDefault()
      zoomAt(wheelZoom(next.current, event.deltaY, event.deltaMode, root.clientHeight), {
        x: event.clientX, y: event.clientY,
      })
    }
    // Safari uses gesture events instead of Ctrl+wheel for trackpad pinches.
    let gestureStart: number | null = null
    const start = (event: Event) => {
      if (!root.querySelector('.pdf-page')) return
      event.preventDefault()
      gestureStart = next.current
    }
    const change = (event: Event) => {
      if (gestureStart === null || !('scale' in event) || typeof event.scale !== 'number') return
      event.preventDefault()
      const point = 'clientX' in event && typeof event.clientX === 'number' &&
        'clientY' in event && typeof event.clientY === 'number'
        ? { x: event.clientX, y: event.clientY } : undefined
      zoomAt(gestureStart * event.scale, point)
    }
    const end = (event: Event) => {
      if (gestureStart !== null) event.preventDefault()
      gestureStart = null
    }
    root.addEventListener('wheel', wheel, { passive: false })
    root.addEventListener('gesturestart', start, { passive: false })
    root.addEventListener('gesturechange', change, { passive: false })
    root.addEventListener('gestureend', end, { passive: false })
    return () => {
      root.removeEventListener('wheel', wheel)
      root.removeEventListener('gesturestart', start)
      root.removeEventListener('gesturechange', change)
      root.removeEventListener('gestureend', end)
      cancelAnimationFrame(frame.current)
      frame.current = 0
      anchor.current = null
    }
  }, [documentKey, scroller, zoomAt])

  const step = (direction: number) => zoomAt(next.current * 1.2 ** direction)
  return { zoom, renderZoom, setZoom, zoomAt, step }
}
