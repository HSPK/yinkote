import { useLayoutEffect, useRef, useState, type RefObject } from 'react'

import type { PdfSearchMatch } from '../lib/pdf-search'
import { pdfSearchRanges } from '../lib/pdf-search-ranges'
import type { ScreenRect } from '../lib/selection-toolbar'
import './pdf-search-overlay.css'

const NO_MATCHES: readonly PdfSearchMatch[] = []

export interface PdfSearchOverlayProps {
  /** The pristine PDF.js text host, not the whole page or the overlay itself. */
  textHost: RefObject<HTMLElement | null>
  matches?: readonly PdfSearchMatch[]
  activeId?: string | null
  /** Increment when a text layer finishes rendering. */
  ready?: unknown
  /** Layout zoom only; changing it never reindexes text or changes the target. */
  zoom?: number
  onActiveRect?: (id: string, rect: ScreenRect) => void
}

interface HighlightRect {
  matchId: string
  left: number
  top: number
  width: number
  height: number
}

/** Mount as a sibling of the scaled text host, inside the positioned PDF page. */
export function PdfSearchOverlay({
  textHost,
  matches = NO_MATCHES,
  activeId,
  ready,
  zoom,
  onActiveRect,
}: PdfSearchOverlayProps) {
  const overlay = useRef<HTMLDivElement>(null)
  const [rects, setRects] = useState<HighlightRect[]>([])

  useLayoutEffect(() => {
    const host = textHost.current
    const target = overlay.current
    if (!host || !target || !matches.length) {
      setRects([])
      return
    }
    const measure = () => {
      const box = target.getBoundingClientRect()
      if (!box.width || !box.height) {
        setRects([])
        return
      }
      const next: HighlightRect[] = []
      for (const { matchId, range } of pdfSearchRanges(host, matches)) {
        for (const rect of range.getClientRects()) {
          if (!rect.width || !rect.height) continue
          next.push({
            matchId,
            left: (rect.left - box.left) / box.width * 100,
            top: (rect.top - box.top) / box.height * 100,
            width: rect.width / box.width * 100,
            height: rect.height / box.height * 100,
          })
        }
      }
      setRects(next)
    }
    measure()
    // Also works when a text layer arrives without a React render.
    const observer = new MutationObserver(measure)
    observer.observe(host, { childList: true, subtree: true, characterData: true })
    const resize = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure)
    resize?.observe(target)
    return () => {
      observer.disconnect()
      resize?.disconnect()
    }
  }, [textHost, matches, ready, zoom])

  useLayoutEffect(() => {
    const active = rects.find((rect) => rect.matchId === activeId)
    const box = overlay.current?.getBoundingClientRect()
    if (!active || !box || !activeId) return
    onActiveRect?.(activeId, {
      left: box.left + active.left / 100 * box.width,
      top: box.top + active.top / 100 * box.height,
      width: active.width / 100 * box.width,
      height: active.height / 100 * box.height,
    })
  }, [rects, activeId, onActiveRect])

  return (
    <div ref={overlay} className="pdf-search-overlay" aria-hidden="true">
      {rects.map((rect, i) => (
        <span
          key={`${rect.matchId}:${i}`}
          className="pdf-search-hit"
          data-search-match={rect.matchId}
          data-current={rect.matchId === activeId ? 'true' : 'false'}
          style={{
            left: `${rect.left}%`,
            top: `${rect.top}%`,
            width: `${rect.width}%`,
            height: `${rect.height}%`,
          }}
        />
      ))}
    </div>
  )
}
