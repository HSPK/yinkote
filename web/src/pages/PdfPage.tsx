import { useEffect, useRef, useState } from 'react'
import type { PDFDocumentProxy, PDFPageProxy, TextLayer } from 'pdfjs-dist'

import { useT } from '../i18n'
import { annotationAt, drawableRects, type Annotation } from '../lib/annotations'
import { failureOf, type Failure } from '../lib/errors'
import { canvasOutput } from './pdfRendering'
import { PdfSearchOverlay } from './PdfSearchOverlay'
import type { PdfSearchMatch } from '../lib/pdf-search'
import type { ScreenRect } from '../lib/selection-toolbar'
import './pdf-page.css'

export interface PdfPageProps {
  doc: PDFDocumentProxy
  pageNumber: number
  zoom: number
  /** Settled bitmap scale. Layout and existing text follow `zoom` immediately. */
  renderZoom?: number
  searchMatches?: readonly PdfSearchMatch[]
  activeMatchId?: string
  onMatchReady?: (id: string, rect: ScreenRect) => void
  selectedAnnotation?: string
  onAnnotationSelect?: (annotation: Annotation, boxes: ScreenRect[]) => void
  annotations: Annotation[]
  /** Reports a selection; it does not act on one. Releasing the mouse used to
   *  write a highlight, so reading with the mouse edited the library. */
  onSelect?: (page: number, pageBox: DOMRect) => void
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
  renderZoom = zoom,
  searchMatches,
  activeMatchId,
  onMatchReady,
  selectedAnnotation,
  onAnnotationSelect,
  annotations,
  onSelect,
  detail,
  reserve,
}: PdfPageProps) {
  const t = useT()
  const canvasRef = useRef<HTMLDivElement>(null)
  const textRef = useRef<HTMLDivElement>(null)
  const wrapRef = useRef<HTMLDivElement>(null)
  const rasterQueue = useRef(Promise.resolve())
  const pointerStart = useRef<{ x: number; y: number } | null>(null)
  const [measured, setMeasured] = useState<{
    doc: PDFDocumentProxy
    pageNumber: number
    page: PDFPageProxy
    viewport: ReturnType<PDFPageProxy['getViewport']>
  } | null>(null)
  const [loadError, setLoadError] = useState<Failure | null>(null)
  const [rasterError, setRasterError] = useState<Failure | null>(null)
  const [textError, setTextError] = useState<Failure | null>(null)
  const enabled = detail !== 'none'
  const full = detail === 'full'
  const current = measured?.doc === doc && measured.pageNumber === pageNumber ? measured : null
  const page = current?.page
  const viewport = current?.viewport
  // Store unscaled dimensions, not the last raster size: even an offscreen
  // page must resize on every layout zoom, including mixed-size documents.
  const points = viewport ?? { width: 0, height: 0 }
  const size = viewport
    ? { width: viewport.width * zoom, height: viewport.height * zoom }
    : reserve
  const error = loadError ?? rasterError ?? textError

  useEffect(() => {
    let live = true
    setLoadError(null)
    if (!enabled || page) return

    void (async () => {
      try {
        const loaded = await doc.getPage(pageNumber)
        if (!live) return
        const base = loaded.getViewport({ scale: 1 })
        canvasOutput(base.width, base.height, 1)
        setMeasured({
          doc,
          pageNumber,
          page: loaded,
          viewport: base,
        })
      } catch (e) {
        if (live) setLoadError(failureOf(e ?? 'PDF page loading failed'))
      }
    })()

    return () => { live = false }
  }, [doc, pageNumber, enabled, page])

  useEffect(() => {
    const host = canvasRef.current
    if (!full || !host) return
    return () => {
      for (const canvas of host.querySelectorAll('canvas')) canvas.width = canvas.height = 0
      host.replaceChildren()
    }
  }, [page, full])

  useEffect(() => {
    setRasterError(null)
    if (!full || !page) return
    let live = true
    let task: ReturnType<PDFPageProxy['render']> | null = null

    // Cancellation is asynchronous. Wait for the previous task to settle,
    // then skip obsolete queued requests before allocating another bitmap.
    rasterQueue.current = rasterQueue.current.then(async () => {
      if (!live) return
      const host = canvasRef.current
      if (!host) return
      const target = page.getViewport({ scale: renderZoom })
      const output = canvasOutput(target.width, target.height, window.devicePixelRatio)
      const canvas = document.createElement('canvas')
      canvas.width = output.width
      canvas.height = output.height
      canvas.setAttribute('aria-hidden', 'true')
      let committed = false
      try {
        const context = canvas.getContext('2d', { alpha: false })
        if (!context) throw new Error('PDF canvas rendering is unavailable')
        task = page.render({
          canvas,
          canvasContext: context,
          viewport: target,
          transform: [output.scaleX, 0, 0, output.scaleY, 0, 0],
        })
        await task.promise
        if (!live) return
        const previous = host.querySelector('canvas')
        host.replaceChildren(canvas)
        committed = true
        if (previous) previous.width = previous.height = 0
      } finally {
        task = null
        if (!committed) canvas.width = canvas.height = 0
      }
    }).catch((e: unknown) => {
      // Only cancellation of an obsolete job is expected. A live job's
      // failures must be visible, whether loading, painting, or extracting text.
      if (live) setRasterError(failureOf(e ?? 'PDF page rendering failed'))
    })

    return () => {
      live = false
      task?.cancel()
    }
  }, [page, full, renderZoom])

  useEffect(() => {
    setTextError(null)
    if (!enabled || !page || !viewport) return
    const host = textRef.current
    if (!host) return
    let live = true
    let task: TextLayer | null = null
    const layer = document.createElement('div')
    layer.className = 'pdf-text-layer'

    void (async () => {
      try {
        const pdfjs = await import('pdfjs-dist')
        if (!live) return
        const text = await page.getTextContent()
        if (!live) return
        task = new pdfjs.TextLayer({ textContentSource: text, container: layer, viewport })
        layer.style.setProperty('--total-scale-factor', String(viewport.scale * viewport.userUnit))
        // The text layer is unrotated internally. Use exact CSS dimensions
        // instead of PDF.js's raster-rounding variables; the outer box scales
        // it without rebuilding spans or destroying the browser's selection.
        const rotated = viewport.rotation % 180 !== 0
        layer.style.width = `${rotated ? viewport.height : viewport.width}px`
        layer.style.height = `${rotated ? viewport.width : viewport.height}px`
        await task.render()
        task = null
        if (live) host.replaceChildren(layer)
      } catch (e) {
        if (live) setTextError(failureOf(e ?? 'PDF text rendering failed'))
      }
    })()

    return () => {
      live = false
      task?.cancel()
      host.replaceChildren()
    }
  }, [page, viewport, enabled])

  return (
    <div
      className="pdf-page"
      data-page={pageNumber}
      ref={wrapRef}
      style={{ width: size.width, height: size.height }}
      onMouseDown={(event) => {
        pointerStart.current = event.button === 0 ? { x: event.clientX, y: event.clientY } : null
      }}
      onClick={(event) => {
        const from = pointerStart.current
        pointerStart.current = null
        if (!onAnnotationSelect || !from || event.button !== 0 || event.detail > 1 ||
          Math.hypot(event.clientX - from.x, event.clientY - from.y) > 4 ||
          window.getSelection()?.toString().trim()) return
        const box = event.currentTarget.getBoundingClientRect()
        if (!box.width || !box.height) return
        const found = annotationAt(annotations, {
          x: (event.clientX - box.left) / box.width,
          y: (event.clientY - box.top) / box.height,
        }, points)
        if (!found) return
        onAnnotationSelect(found, drawableRects(found, points).map((rect) => ({
          left: box.left + rect.x * box.width,
          top: box.top + rect.y * box.height,
          width: rect.w * box.width,
          height: rect.h * box.height,
        })))
      }}
      onMouseUp={() => {
        const box = wrapRef.current?.getBoundingClientRect()
        if (box) onSelect?.(pageNumber, box)
      }}
    >
      {/* The canvas is dropped when the page is far away — a hundred canvases
          at device resolution is hundreds of megabytes of pixels for pages
          nobody is looking at. The box it leaves behind is not. */}
      {full && <div className="pdf-canvas" ref={canvasRef} />}

      <div className="pdf-highlights">
        {annotations.flatMap((a) =>
          drawableRects(a, points).map((r, i) => (
            <span
              key={`${a.key}-${i}`}
              className="pdf-highlight"
              data-kind={a.kind}
              data-colour={a.colour}
              data-annotation={a.key}
              data-active={a.key === selectedAnnotation || undefined}
              title={a.comment || a.text}
              style={{
                left: `${r.x * 100}%`,
                top: `${r.y * 100}%`,
                width: `${r.w * 100}%`,
                height: `${r.h * 100}%`,
              }}
            />
          )),
        )}
      </div>

      {enabled && (
        <div
          className="pdf-text"
          ref={textRef}
          style={{ width: points.width, height: points.height, transform: `scale(${zoom})` }}
        />
      )}
      {enabled && searchMatches?.length ? (
        <PdfSearchOverlay textHost={textRef} matches={searchMatches} activeId={activeMatchId}
          ready={page} zoom={zoom} onActiveRect={onMatchReady} />
      ) : null}
      {enabled && error && (
        <div className="pdf-render-error" role="alert" title={error.detail}>
          {t('reader.unsupported')}
        </div>
      )}
      <span className="pdf-number">{pageNumber}</span>
    </div>
  )
}
