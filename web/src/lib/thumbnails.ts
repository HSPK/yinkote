/**
 * Page thumbnails: fetch one, or draw it and hand it back.
 *
 * The server keeps no rasteriser — rendering a PDF page in Rust means pdfium or
 * mupdf, a large native dependency built per platform, for a program whose
 * premise is that a user can install it. The browser already has pdf.js, so the
 * split is: the server caches, the client renders.
 *
 * A 404 from the cache is therefore not an error. It is the instruction to draw
 * the page and PUT it, after which every other tab and every later session gets
 * it from disk for the cost of a static file.
 */

import { api } from '../api/client'

/** Widths the server will cache. Anything else is refused. */
export const THUMB_WIDTHS = [96, 240, 480, 960] as const
export type ThumbWidth = (typeof THUMB_WIDTHS)[number]

/**
 * Draw one page of a PDF to a PNG blob.
 *
 * Scaled by width rather than a zoom factor: the cache is keyed by width, and a
 * factor would produce a different number of pixels for every page size in the
 * library.
 */
export async function renderPage(url: string, page: number, width: number): Promise<Blob> {
  const pdfjs = await import('pdfjs-dist')
  pdfjs.GlobalWorkerOptions.workerSrc = new URL(
    'pdfjs-dist/build/pdf.worker.min.mjs',
    import.meta.url,
  ).toString()

  const task = pdfjs.getDocument({ url })
  try {
    const doc = await task.promise
    // A page past the end is not worth an exception here; the caller asked for
    // a picture and the first page is a better answer than a crash.
    const target = Math.min(Math.max(1, page), doc.numPages)
    const rendered = await doc.getPage(target)

    const unscaled = rendered.getViewport({ scale: 1 })
    const viewport = rendered.getViewport({ scale: width / unscaled.width })

    const canvas = document.createElement('canvas')
    canvas.width = Math.round(viewport.width)
    canvas.height = Math.round(viewport.height)
    const context = canvas.getContext('2d')
    if (!context) throw new Error('no 2d context')

    await rendered.render({ canvas, canvasContext: context, viewport }).promise

    return await new Promise<Blob>((resolve, reject) => {
      canvas.toBlob(
        (blob) => (blob ? resolve(blob) : reject(new Error('could not encode the page'))),
        'image/png',
      )
    })
  } finally {
    // The task owns the worker and the network requests, so it is what must be
    // torn down — not the document.
    void task.destroy()
  }
}

/**
 * The cached thumbnail for a page, drawing and storing one if there is none.
 *
 * Returns null when there is nothing to draw — no file, or a file that is not a
 * PDF — which the caller shows as an absence rather than an error. A library is
 * full of items with no attachment, and that is not a fault.
 */
export async function thumbnailFor(
  library: number,
  attachmentKey: string,
  page: number,
  width: ThumbWidth,
): Promise<Blob | null> {
  const cached = await api.thumbnails.get(library, attachmentKey, page, width)
  if (cached) return cached

  try {
    const drawn = await renderPage(api.files.url(library, attachmentKey), page, width)
    // Stored, not awaited for correctness: the picture is already in hand, and
    // a cache write that fails only costs the next reader a redraw.
    void api.thumbnails.put(library, attachmentKey, page, width, drawn).catch(() => {})
    return drawn
  } catch {
    return null
  }
}
