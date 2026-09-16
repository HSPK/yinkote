import { useEffect, useMemo, useState } from 'react'
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
  const [state, setState] = useState<{
    url: string | null
    doc: PDFDocumentProxy | null
    error: Failure | null
  }>({ url: null, doc: null, error: null })

  useEffect(() => {
    setState({ url, doc: null, error: null })
    if (!url) return
    let live = true
    // The loading task, not the document, owns the worker and the network
    // requests — so it is the thing that must be torn down.
    let task: PDFDocumentLoadingTask | null = null

    void (async () => {
      try {
        const pdfjs = await import('pdfjs-dist')
        // Importing the large module may outlive this attachment or component.
        // Do not create a worker/loading task after its cleanup already ran.
        if (!live) return
        pdfjs.GlobalWorkerOptions.workerSrc = new URL(
          'pdfjs-dist/build/pdf.worker.min.mjs',
          import.meta.url,
        ).toString()

        task = pdfjs.getDocument({ url })
        const loaded = await task.promise
        if (!live) return
        setState({ url, doc: loaded, error: null })
      } catch (e) {
        if (live) setState({ url, doc: null, error: failureOf(e ?? 'PDF loading failed') })
      }
    })()

    return () => {
      live = false
      void task?.destroy().catch((e: unknown) => {
        console.error('Failed to destroy PDF loading task', e)
      })
    }
  }, [url])

  // Effects run after paint. Key the result as well so a new attachment cannot
  // briefly render the old document or its error before the effect resets it.
  const { doc, error } = state.url === url ? state : { doc: null, error: null }
  const pages = useMemo(
    () => doc ? Array.from({ length: doc.numPages }, (_, i) => i + 1) : [],
    [doc],
  )
  return { doc, pages, error }
}
