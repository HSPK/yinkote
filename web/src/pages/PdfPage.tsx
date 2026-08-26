import { useEffect, useRef, useState } from 'react'
import type { PDFDocumentProxy } from 'pdfjs-dist'

import type { Annotation } from '../lib/annotations'

export interface PdfPageProps {
  doc: PDFDocumentProxy
  pageNumber: number
  zoom: number
  annotations: Annotation[]
  onHighlight: (page: number, pageBox: DOMRect) => void
  onRemove: (key: string) => void
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
  onHighlight,
  onRemove,
}: PdfPageProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const textRef = useRef<HTMLDivElement>(null)
  const wrapRef = useRef<HTMLDivElement>(null)
  const [size, setSize] = useState({ width: 0, height: 0 })

  useEffect(() => {
    let live = true
    let task: { cancel: () => void } | null = null

    void (async () => {
      const page = await doc.getPage(pageNumber)
      if (!live) return

      // Render at device resolution and scale back down with CSS, or the page
      // is soft on every screen made in the last decade.
      const ratio = window.devicePixelRatio || 1
      const viewport = page.getViewport({ scale: zoom })
      const canvas = canvasRef.current
      const context = canvas?.getContext('2d')
      if (!canvas || !context) return

      canvas.width = Math.floor(viewport.width * ratio)
      canvas.height = Math.floor(viewport.height * ratio)
      setSize({ width: viewport.width, height: viewport.height })

      const render = page.render({
        canvas,
        canvasContext: context,
        viewport,
        transform: ratio === 1 ? undefined : [ratio, 0, 0, ratio, 0, 0],
      })
      task = render
      await render.promise.catch(() => {})
      if (!live) return

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
  }, [doc, pageNumber, zoom])

  return (
    <div
      className="pdf-page"
      data-page={pageNumber}
      ref={wrapRef}
      style={{ width: size.width || undefined, height: size.height || undefined }}
      onMouseUp={() => {
        const box = wrapRef.current?.getBoundingClientRect()
        if (box) onHighlight(pageNumber, box)
      }}
    >
      <canvas ref={canvasRef} style={{ width: size.width, height: size.height }} />

      <div className="pdf-highlights">
        {annotations.flatMap((a) =>
          a.rects.map((r, i) => (
            <span
              key={`${a.key}-${i}`}
              className="pdf-highlight"
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

      <div className="pdf-text" ref={textRef} />
      <span className="pdf-number">{pageNumber}</span>
    </div>
  )
}
