import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { PDFDocumentProxy } from 'pdfjs-dist'

import { usePdf } from './usePdf'

const pdfjs = vi.hoisted(() => {
  let release!: () => void
  const ready = new Promise<void>((resolve) => { release = resolve })
  return { ready, release, getDocument: vi.fn(), GlobalWorkerOptions: { workerSrc: '' } }
})

vi.mock('pdfjs-dist', async () => {
  await pdfjs.ready
  return { getDocument: pdfjs.getDocument, GlobalWorkerOptions: pdfjs.GlobalWorkerOptions }
})

function loading() {
  let resolve!: (doc: PDFDocumentProxy) => void
  let reject!: (error: Error) => void
  const promise = new Promise<PDFDocumentProxy>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject, destroy: vi.fn(async () => {}) }
}

let container: HTMLElement
let root: Root
const seen: Array<ReturnType<typeof usePdf> & { url: string | null }> = []
function Probe({ url }: { url: string | null }) {
  seen.push({ url, ...usePdf(url) })
  return null
}

async function mount(url: string | null) {
  await act(async () => { root.render(<Probe url={url} />) })
}

beforeEach(() => {
  seen.length = 0
  pdfjs.getDocument.mockReset()
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
  vi.restoreAllMocks()
})

describe('PDF loading lifecycle', () => {
  it('does not create a loading task when a delayed module import finishes after unmount', async () => {
    await mount('late.pdf')
    act(() => root.render(null))
    await act(async () => {
      pdfjs.release()
      await vi.dynamicImportSettled()
    })
    expect(pdfjs.getDocument).not.toHaveBeenCalled()
  })

  it('does not expose the old document for even the first render of another attachment', async () => {
    const first = loading()
    const second = loading()
    pdfjs.getDocument.mockReturnValueOnce(first).mockReturnValueOnce(second)
    await mount('first.pdf')
    const doc = { numPages: 3 } as PDFDocumentProxy
    await act(async () => first.resolve(doc))
    expect(seen.at(-1)?.doc).toBe(doc)
    expect(seen.at(-1)?.pages).toEqual([1, 2, 3])

    seen.length = 0
    await mount('second.pdf')
    expect(first.destroy).toHaveBeenCalledTimes(1)
    expect(seen[0]?.url).toBe('second.pdf')
    expect(seen.every((snapshot) => snapshot.doc === null && snapshot.error === null)).toBe(true)
    expect(seen.at(-1)?.pages).toEqual([])
  })

  it('clears an old failure immediately on another attachment or no attachment', async () => {
    const first = loading()
    const second = loading()
    pdfjs.getDocument.mockReturnValueOnce(first).mockReturnValueOnce(second)
    await mount('broken.pdf')
    await act(async () => first.reject(new Error('Broken PDF')))
    expect(seen.at(-1)?.error?.detail).toBe('Broken PDF')

    seen.length = 0
    await mount('next.pdf')
    expect(seen.every((snapshot) => snapshot.error === null)).toBe(true)
    await act(async () => second.reject(new Error('Another broken PDF')))
    seen.length = 0
    await mount(null)
    expect(seen.every((snapshot) => snapshot.error === null && snapshot.doc === null)).toBe(true)
    expect(second.destroy).toHaveBeenCalledTimes(1)
  })

  it('ignores an obsolete document and observes its cancellation rejection', async () => {
    const first = loading()
    const second = loading()
    first.destroy.mockImplementation(async () => first.reject(new Error('Loading cancelled')))
    pdfjs.getDocument.mockReturnValueOnce(first).mockReturnValueOnce(second)
    await mount('first.pdf')
    await mount('second.pdf')
    expect(seen.at(-1)?.error).toBeNull()
    const current = { numPages: 1 } as PDFDocumentProxy
    await act(async () => second.resolve(current))
    expect(seen.at(-1)?.doc).toBe(current)
  })

  it('does not replace the current document when an obsolete load resolves late', async () => {
    const first = loading()
    const second = loading()
    pdfjs.getDocument.mockReturnValueOnce(first).mockReturnValueOnce(second)
    await mount('first.pdf')
    await mount('second.pdf')
    const current = { numPages: 1 } as PDFDocumentProxy
    await act(async () => second.resolve(current))
    await act(async () => first.resolve({ numPages: 99 } as PDFDocumentProxy))
    expect(seen.at(-1)?.doc).toBe(current)
    expect(seen.at(-1)?.pages).toEqual([1])
  })

  it('observes and reports a failed loading-task teardown', async () => {
    const task = loading()
    task.destroy.mockRejectedValue(new Error('Worker teardown failed'))
    pdfjs.getDocument.mockReturnValueOnce(task)
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    await mount('worker.pdf')
    await act(async () => root.render(null))
    expect(error).toHaveBeenCalledWith('Failed to destroy PDF loading task', expect.any(Error))
  })
})
