import { useEffect, useRef, useState } from 'react'
import type { PDFDocumentProxy, RenderTask } from 'pdfjs-dist'

import { useT } from '../i18n'
import { Thumbnail } from './Thumbnail'
import { failureOf, type Failure } from '../lib/errors'

/** Reuse the reader's document and worker instead of opening the same PDF for
 *  every thumbnail. Only visible cells own canvases. */
function PageThumbnail({ doc, page }: { doc: PDFDocumentProxy; page: number }) {
  const t = useT()
  const container = useRef<HTMLSpanElement>(null)
  const [error, setError] = useState<Failure | null>(null)
  useEffect(() => {
    const host = container.current
    let live = true
    let task: RenderTask | undefined
    let canvas: HTMLCanvasElement | undefined
    let committed = false
    setError(null)
    void (async () => {
      try {
        const source = await doc.getPage(page)
        if (!live) return
        const base = source.getViewport({ scale: 1 })
        if (!Number.isFinite(base.width) || !Number.isFinite(base.height) || base.width <= 0 || base.height <= 0) {
          throw new Error('Invalid PDF page dimensions')
        }
        const dpr = window.devicePixelRatio
        const ratio = Number.isFinite(dpr) && dpr > 0 ? Math.min(2, dpr) : 1
        // A very tall page must not allocate a full-height bitmap for a rail cell.
        const scale = Math.min(96 / base.width, 128 / base.height)
        const viewport = source.getViewport({ scale: scale * ratio })
        canvas = document.createElement('canvas')
        canvas.width = Math.max(1, Math.floor(viewport.width))
        canvas.height = Math.max(1, Math.floor(viewport.height))
        canvas.style.width = `${base.width * scale}px`
        canvas.style.height = `${base.height * scale}px`
        canvas.style.margin = '0 auto'
        const context = canvas.getContext('2d', { alpha: false })
        if (!context) throw new Error('PDF thumbnail canvas rendering is unavailable')
        task = source.render({
          canvas,
          canvasContext: context,
          viewport,
          transform: [canvas.width / viewport.width, 0, 0, canvas.height / viewport.height, 0, 0],
        })
        await task.promise
        if (!live || !host) return
        host.replaceChildren(canvas)
        committed = true
      } catch (error) {
        if (live) setError(failureOf(error ?? 'PDF thumbnail rendering failed'))
      } finally {
        task = undefined
        // Cancellation may settle after the cell disappeared. Release only
        // then, and also release canvases whose render could not even start.
        if (canvas && !committed) canvas.width = canvas.height = 0
      }
    })()
    return () => {
      live = false
      task?.cancel()
      if (canvas && !task) canvas.width = canvas.height = 0
      host?.replaceChildren()
    }
  }, [doc, page])
  return <span className="page-raster" role="img"
    aria-label={t('detail.thumbnailAlt', { page })}>
    <span ref={container} />
    {error && <span role="alert" title={error.detail}>{t('reader.unsupported')}</span>}
  </span>
}

/**
 * A rail of page thumbnails down the side of the reader.
 *
 * The fastest way to find a figure in a forty-page paper, and the reason the
 * server grew a thumbnail cache: the pictures are drawn once, by whichever
 * browser opens the document first, and every later reader gets a static file.
 *
 * Only what is on screen is asked for. A three-hundred-page thesis would
 * otherwise render three hundred pages before showing the first one — and each
 * miss is a full pdf.js page render, so the cost of being eager here is not the
 * usual "a few extra requests".
 */
export function PageRail({
  library,
  attachmentKey,
  pages,
  current,
  onJump,
  doc,
}: {
  library: number
  attachmentKey: string
  pages: number[]
  current: number
  onJump: (page: number) => void
  doc?: PDFDocumentProxy
}) {
  const t = useT()
  const railRef = useRef<HTMLDivElement>(null)
  const [visible, setVisible] = useState<Set<number>>(new Set())

  useEffect(() => {
    const root = railRef.current
    if (!root) return
    let live = true
    setVisible(new Set())
    const observer = new IntersectionObserver(
      (entries) => {
        if (!live) return
        setVisible((was) => {
          const now = new Set(was)
          for (const entry of entries) {
            const page = Number((entry.target as HTMLElement).dataset.page)
            if (entry.isIntersecting) now.add(page)
            else now.delete(page)
          }
          return now
        })
      },
      { root, rootMargin: '200px 0px' },
    )
    root.querySelectorAll('[data-page]').forEach((cell) => observer.observe(cell))
    return () => {
      live = false
      observer.disconnect()
    }
  }, [pages.length, attachmentKey, library, doc])

  // Follow the reader: scrolling the document moves the rail, so the current
  // page is always in view without the user having to hunt for it.
  useEffect(() => {
    const root = railRef.current
    const cell = root?.querySelector<HTMLElement>(`[data-page="${current}"]`)
    if (!root || !cell) return
    const bounds = root.getBoundingClientRect()
    const box = cell.getBoundingClientRect()
    if (box.top < bounds.top) root.scrollTop += box.top - bounds.top
    else if (box.bottom > bounds.bottom) root.scrollTop += box.bottom - bounds.bottom
  }, [current])

  return (
    <div className="page-rail" ref={railRef} aria-label={t('reader.pages')}>
      {pages.map((page) => (
        <button
          key={page}
          className="page-cell"
          data-page={page}
          data-active={page === current}
          onClick={() => onJump(page)}
          title={t('reader.goToPage', { page })}
        >
          <span className="page-shot">
            {visible.has(page) && (doc ? <PageThumbnail doc={doc} page={page} /> : (
              <Thumbnail
                library={library}
                attachmentKey={attachmentKey}
                page={page}
                width={96}
              />
            ))}
          </span>
          <span className="page-number">{page}</span>
        </button>
      ))}
    </div>
  )
}
