import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react'
import { pdfSelection } from '../lib/pdf-selection'
import type { ScreenRect } from '../lib/selection-toolbar'

interface Paint {
  viewport: ScreenRect
  rects: ScreenRect[]
}

const sameBox = (a: ScreenRect, b: ScreenRect) =>
  a.left === b.left && a.top === b.top && a.width === b.width && a.height === b.height

/** Native selection supplies the range; an isolated, frame-coalesced layer
 *  paints it continuously without changing any PDF text nodes or caret offsets. */
export function PdfSelectionOverlay({ root, active, ready, zoom }: {
  root: RefObject<HTMLElement | null>
  active: boolean
  ready: unknown
  zoom: number
}) {
  const [paint, setPaint] = useState<Paint | null>(null)
  const frame = useRef(0)

  useEffect(() => {
    const scroller = root.current
    if (!active || !scroller) {
      setPaint(null)
      return
    }
    const measure = () => {
      frame.current = 0
      const selection = window.getSelection()
      const chosen = selection ? pdfSelection(scroller, selection, true) : []
      const viewport = scroller.getBoundingClientRect()
      const rects: ScreenRect[] = []
      for (const selected of chosen) {
        const page = scroller.querySelector<HTMLElement>(`.pdf-page[data-page="${selected.page}"]`)
        if (!page) continue
        const box = page.getBoundingClientRect()
        for (const rect of selected.rects) {
          const left = Math.max(viewport.left, box.left + rect.x * box.width)
          const top = Math.max(viewport.top, box.top + rect.y * box.height)
          const right = Math.min(viewport.right, box.left + (rect.x + rect.w) * box.width)
          const bottom = Math.min(viewport.bottom, box.top + (rect.y + rect.h) * box.height)
          if (right > left && bottom > top) {
            rects.push({ left: left - viewport.left, top: top - viewport.top, width: right - left, height: bottom - top })
          }
        }
      }
      const next = rects.length ? {
        viewport: { left: viewport.left, top: viewport.top, width: viewport.width, height: viewport.height },
        rects,
      } : null
      setPaint((old) => {
        if (!old || !next) return old === next ? old : next
        return sameBox(old.viewport, next.viewport) && old.rects.length === next.rects.length &&
          old.rects.every((rect, i) => sameBox(rect, next.rects[i]!)) ? old : next
      })
    }
    const schedule = () => {
      if (!frame.current) frame.current = requestAnimationFrame(measure)
    }
    document.addEventListener('selectionchange', schedule)
    scroller.addEventListener('scroll', schedule, { passive: true })
    window.addEventListener('resize', schedule)
    const mutation = new MutationObserver((records) => {
      if (records.some((record) => record.target instanceof Element && record.target.closest('.pdf-page'))) schedule()
    })
    mutation.observe(scroller, { childList: true, subtree: true, attributes: true, attributeFilter: ['style'] })
    const resize = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(schedule)
    resize?.observe(scroller)
    schedule()
    return () => {
      cancelAnimationFrame(frame.current)
      frame.current = 0
      mutation.disconnect()
      resize?.disconnect()
      document.removeEventListener('selectionchange', schedule)
      scroller.removeEventListener('scroll', schedule)
      window.removeEventListener('resize', schedule)
    }
  }, [root, active, ready, zoom])

  // Apply the native-paint mask in the same commit as the replacement layer.
  useLayoutEffect(() => {
    const scroller = root.current
    if (active && paint) scroller?.setAttribute('data-continuous-selection', 'true')
    return () => scroller?.removeAttribute('data-continuous-selection')
  }, [active, Boolean(paint), root])

  if (!active || !paint) return null
  return (
    <div className="pdf-live-selection" aria-hidden="true" style={paint.viewport}>
      {paint.rects.map((rect, i) => (
        <span className="pdf-live-selection-line" key={i} style={rect} />
      ))}
    </div>
  )
}
