import { useEffect, useRef, useState } from 'react'
import type { PDFDocumentProxy } from 'pdfjs-dist'

import { drawableRects, type Annotation } from '../lib/annotations'

export interface PdfPageProps {
  doc: PDFDocumentProxy
  pageNumber: number
  zoom: number
  annotations: Annotation[]
  /** Reports a selection; it does not act on one. Releasing the mouse used to
   *  write a highlight, so reading with the mouse edited the library. */
  onSelect: (page: number, pageBox: DOMRect) => void
  onRemove: (key: string) => void
  /** How much of this page to build.
   *
   *  - `full`: the picture and the text over it. Near the viewport.
   *  - `text`: the text layer only. Far away, but a search is running — and
   *    find works over the rendered spans, so a page with no text layer is a
   *    page the search silently skips. Spans are cheap; a canvas at device
   *    resolution is several megabytes.
   *  - `none`: neither.
   *
   *  A page keeps its box in every case. The scrollbar has to mean something,
   *  and a page that collapsed when it left the screen would drag everything
   *  below it upwards under the reader's eyes. */
  detail: 'full' | 'text' | 'none'
  /** What to reserve before this page has ever been measured. Page one's size,
   *  which is every page's size in all but a handful of documents. */
  reserve: { width: number; height: number }
}

/**
 * One rendered page, with its text layer and highlights.
 *
 * The text layer is what makes selection possible: pdf.js draws the page to a
 * canvas, which has no text at all, so a transparent layer of positioned spans
 * is laid over it. Highlights sit between the two — above the picture, below
 * the text — so that selecting still works over a highlighted passage.
 */
export function PdfPage({
  doc,
  pageNumber,
  zoom,
  annotations,
  onSelect,
  onRemove,
  detail,
  reserve,
}: PdfPageProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const textRef = useRef<HTMLDivElement>(null)
  const wrapRef = useRef<HTMLDivElement>(null)
  const [size, setSize] = useState({ width: 0, height: 0 })
  // The page's own size in points, which is the only thing that can place an
  // imported highlight: it was recorded in points and means nothing without it.
  const [points, setPoints] = useState({ width: 0, height: 0 })

  useEffect(() => {
    if (detail === 'none') return
    let live = true
    let task: { cancel: () => void } | null = null

    void (async () => {
      const page = await doc.getPage(pageNumber)
      if (!live) return

      const viewport = page.getViewport({ scale: zoom })
      setSize({ width: viewport.width, height: viewport.height })
      const unscaled = page.getViewport({ scale: 1 })
      setPoints({ width: unscaled.width, height: unscaled.height })

      if (detail === 'full') {
        // Render at device resolution and scale back down with CSS, or the
        // page is soft on every screen made in the last decade.
        const ratio = window.devicePixelRatio || 1
        const canvas = canvasRef.current
        const context = canvas?.getContext('2d')
        if (!canvas || !context) return

        canvas.width = Math.floor(viewport.width * ratio)
        canvas.height = Math.floor(viewport.height * ratio)
        const render = page.render({
          canvas,
          canvasContext: context,
          viewport,
          transform: ratio === 1 ? undefined : [ratio, 0, 0, ratio, 0, 0],
        })
        task = render
        await render.promise.catch(() => {})
        if (!live) return
      }

      const layer = textRef.current
      if (layer) {
        layer.replaceChildren()
        const pdfjs = await import('pdfjs-dist')
        const text = await page.getTextContent()
        if (!live) return
        await new pdfjs.TextLayer({ textContentSource: text, container: layer, viewport }).render()
      }
    })()

    return () => {
      live = false
      task?.cancel()
    }
  }, [doc, pageNumber, zoom, detail])

  return (
    <div
      className="pdf-page"
      data-page={pageNumber}
      ref={wrapRef}
      // Measured once drawn, reserved before that. Never unset: a page that has
      // been seen keeps its size when it scrolls away.
      style={{ width: size.width || reserve.width, height: size.height || reserve.height }}
      onMouseUp={() => {
        const box = wrapRef.current?.getBoundingClientRect()
        if (box) onSelect(pageNumber, box)
      }}
    >
      {/* The canvas is dropped when the page is far away — a hundred canvases
          at device resolution is hundreds of megabytes of pixels for pages
          nobody is looking at. The box it leaves behind is not. */}
      {detail === 'full' && (
        <canvas ref={canvasRef} style={{ width: size.width, height: size.height }} />
      )}

      <div className="pdf-highlights">
        {annotations.flatMap((a) =>
          drawableRects(a, points).map((r, i) => (
            <span
              key={`${a.key}-${i}`}
              className="pdf-highlight"
              data-kind={a.kind}
              data-colour={a.colour}
              title={a.comment || a.text}
              style={{
                left: `${r.x * 100}%`,
                top: `${r.y * 100}%`,
                width: `${r.w * 100}%`,
                height: `${r.h * 100}%`,
              }}
              onDoubleClick={() => onRemove(a.key)}
            />
          )),
        )}
      </div>

      {detail !== 'none' && <div className="pdf-text" ref={textRef} />}
      <span className="pdf-number">{pageNumber}</span>
    </div>
  )
}
