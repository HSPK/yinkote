import { useEffect, useState } from 'react'
import { failureOf, type Failure } from '../lib/errors'
import type { PDFDocumentLoadingTask, PDFDocumentProxy } from 'pdfjs-dist'

/**
 * Loads a PDF.
 *
 * The worker is wired here rather than at module scope so that importing the
 * reader does not pull pdf.js into the initial bundle: most sessions never open
 * a paper, and the viewer is by far the heaviest thing in the app.
 */
export function usePdf(url: string | null) {
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null)
  const [error, setError] = useState<Failure | null>(null)

  useEffect(() => {
    if (!url) {
      setDoc(null)
      return
    }
    let live = true
    // The loading task, not the document, owns the worker and the network
    // requests — so it is the thing that must be torn down.
    let task: PDFDocumentLoadingTask | null = null

    void (async () => {
      try {
        const pdfjs = await import('pdfjs-dist')
        pdfjs.GlobalWorkerOptions.workerSrc = new URL(
          'pdfjs-dist/build/pdf.worker.min.mjs',
          import.meta.url,
        ).toString()

        task = pdfjs.getDocument({ url })
        const loaded = await task.promise
        if (!live) return
        setDoc(loaded)
        setError(null)
      } catch (e) {
        if (live) setError(failureOf(e))
      }
    })()

    return () => {
      live = false
      setDoc(null)
      void task?.destroy()
    }
  }, [url])

  const pages = doc ? Array.from({ length: doc.numPages }, (_, i) => i + 1) : []
  return { doc, pages, error }
}
