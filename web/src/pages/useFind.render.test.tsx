import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { PDF_SEARCH_CONCURRENCY, type PdfSearchDocument, type PdfTextRun } from '../lib/pdf-search'
import { useFind } from './useFind'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

function documentWithPages(count: number) {
  const pending = Array.from({ length: count }, () => deferred<{ items: PdfTextRun[] }>())
  const readers = pending.map((page) => vi.fn(() => page.promise))
  const getPage = vi.fn(async (number: number) => ({ getTextContent: readers[number - 1]! }))
  return { doc: { numPages: count, getPage }, pending, readers }
}

let container: HTMLDivElement
let root: Root
let result: ReturnType<typeof useFind>

function Harness({ doc, query, zoom = 1 }: {
  doc: PdfSearchDocument | null
  query: string
  zoom?: number
}) {
  result = useFind(doc, query)
  return <div data-total={result.total} data-index={result.index} data-zoom={zoom} />
}

async function render(doc: PdfSearchDocument | null, query: string, zoom = 1) {
  await act(async () => { root.render(<Harness doc={doc} query={query} zoom={zoom} />) })
}

beforeEach(() => {
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

describe('document-backed find', () => {
  it('updates a twelve-page document from three early hits to twenty-nine without DOM readiness signals', async () => {
    const { doc, pending } = documentWithPages(12)
    pending.slice(0, 3).forEach((page) => page.resolve({ items: [{ str: 'mental' }] }))
    await render(doc, 'mental')
    expect(result.total).toBe(3)
    expect(result.loading).toBe(true)
    const active = result.active
    await act(async () => {
      pending.slice(3).forEach((page, i) => {
        page.resolve({ items: [{ str: Array(i === 8 ? 2 : 3).fill('mental').join(' ') }] })
      })
    })
    expect(result.total).toBe(29)
    expect(result.loading).toBe(false)
    expect(result.active).toBe(active)
    act(() => result.go(-1))
    expect(result.index).toBe(29)
    expect(result.active!.pageNumber).toBe(12)
    act(() => result.go(1))
    expect(result.index).toBe(1)
    expect(result.active!.pageNumber).toBe(1)
  })

  it('counts asynchronously arriving pages without rendering any PDF text or canvas', async () => {
    const { doc, pending } = documentWithPages(3)
    await render(doc, 'mental')
    expect(result.loading).toBe(true)
    expect(result.total).toBe(0)
    await act(async () => {
      pending[1]!.resolve({ items: [{ str: 'men' }, { str: 'tal' }] })
    })
    expect(result.total).toBe(1)
    expect(result.active!.pageNumber).toBe(2)
    const active = result.active
    await act(async () => {
      pending[0]!.resolve({ items: [{ str: 'mental and mental' }] })
      pending[2]!.resolve({ items: [{ str: 'mental health' }] })
    })
    expect(result.loading).toBe(false)
    expect(result.total).toBe(4)
    expect(result.index).toBe(3)
    expect(result.active).toBe(active)
    expect([...result.matchesByPage.keys()]).toEqual([1, 2, 3])
    expect(container.querySelector('.pdf-text, canvas')).toBeNull()
    act(() => result.go(1))
    expect(result.active!.pageNumber).toBe(3)
  })

  it('keeps advancing with the same result count and does not reset on zoom or rerender', async () => {
    const { doc, pending } = documentWithPages(1)
    pending[0]!.resolve({ items: [{ str: 'mental mental mental' }] })
    await render(doc, 'mental')
    expect(result.index).toBe(1)
    act(() => result.go(1))
    expect(result.index).toBe(2)
    await render(doc, 'mental', 1.7)
    expect(result.index).toBe(2)
    act(() => result.go(1))
    expect(result.index).toBe(3)
    act(() => result.go(1))
    expect(result.index).toBe(1)
    act(() => result.go(-1))
    expect(result.index).toBe(3)
  })

  it('reuses document text across keystrokes and query clearing', async () => {
    const { doc, pending, readers } = documentWithPages(2)
    await render(doc, '')
    expect(doc.getPage).not.toHaveBeenCalled()
    expect(result.loading).toBe(false)
    pending.forEach((page) => page.resolve({ items: [{ str: 'mental health' }] }))
    await render(doc, 'm')
    await render(doc, 'ment')
    await render(doc, 'mental')
    expect(result.total).toBe(2)
    act(() => result.go(1))
    expect(result.index).toBe(2)
    await render(doc, 'mental health')
    expect(result.index).toBe(1)
    expect(result.total).toBe(2)
    await render(doc, '')
    expect(result.total).toBe(0)
    expect(result.index).toBe(0)
    expect(result.active).toBeNull()
    expect(result.matchesByPage.size).toBe(0)
    await render(doc, 'mental')
    expect(result.total).toBe(2)
    expect(doc.getPage).toHaveBeenCalledTimes(2)
    readers.forEach((read) => expect(read).toHaveBeenCalledTimes(1))
  })

  it('bounds concurrency, stops queueing when cleared, and resumes cached work', async () => {
    const count = PDF_SEARCH_CONCURRENCY + 3
    const { doc, pending, readers } = documentWithPages(count)
    await render(doc, 'mental')
    expect(doc.getPage).toHaveBeenCalledTimes(PDF_SEARCH_CONCURRENCY)
    await render(doc, '')
    await act(async () => {
      pending.slice(0, PDF_SEARCH_CONCURRENCY).forEach((page) =>
        page.resolve({ items: [{ str: 'mental' }] }))
    })
    expect(doc.getPage).toHaveBeenCalledTimes(PDF_SEARCH_CONCURRENCY)
    expect(result.total).toBe(0)
    await render(doc, 'mental')
    expect(result.total).toBe(PDF_SEARCH_CONCURRENCY)
    expect(doc.getPage).toHaveBeenCalledTimes(count)
    await act(async () => {
      pending.slice(PDF_SEARCH_CONCURRENCY).forEach((page) =>
        page.resolve({ items: [{ str: 'mental' }] }))
    })
    expect(result.loading).toBe(false)
    expect(result.total).toBe(count)
    readers.forEach((read) => expect(read).toHaveBeenCalledTimes(1))
  })

  it('does not let late old-document work replace the current document or queue more work', async () => {
    const old = documentWithPages(PDF_SEARCH_CONCURRENCY + 1)
    const current = documentWithPages(1)
    await render(old.doc, 'mental')
    current.pending[0]!.resolve({ items: [{ str: 'mental mental' }] })
    await render(current.doc, 'mental')
    expect(result.total).toBe(2)
    act(() => result.go(1))
    const active = result.active
    await act(async () => {
      old.pending.slice(0, PDF_SEARCH_CONCURRENCY).forEach((page) =>
        page.resolve({ items: [{ str: 'mental' }] }))
    })
    expect(result.total).toBe(2)
    expect(result.index).toBe(2)
    expect(result.active).toBe(active)
    expect(old.doc.getPage).toHaveBeenCalledTimes(PDF_SEARCH_CONCURRENCY)
  })

  it('uses the latest query when text arrives and ignores arrivals after unmount', async () => {
    const { doc, pending } = documentWithPages(PDF_SEARCH_CONCURRENCY + 1)
    await render(doc, 'mental')
    await render(doc, 'health')
    await act(async () => { pending[0]!.resolve({ items: [{ str: 'mental health health' }] }) })
    expect(result.total).toBe(2)
    expect(result.active!.start.offset).toBe(7)
    act(() => root.render(null))
    await act(async () => {
      pending.slice(1).forEach((page) => page.resolve({ items: [{ str: 'health' }] }))
    })
    expect(container.textContent).toBe('')
  })

  it('surfaces extraction errors rather than treating a failed page as no matches', async () => {
    const { doc, pending } = documentWithPages(2)
    await render(doc, 'mental')
    await act(async () => {
      pending[0]!.reject(new Error('Unable to read PDF bytes'))
      pending[1]!.resolve({ items: [{ str: 'mental' }] })
    })
    expect(result.loading).toBe(false)
    expect(result.total).toBe(1)
    expect(result.error?.message).toContain('page 1: Unable to read PDF bytes')
    await render(doc, '')
    expect(result.error).toBeNull()
  })

  it('surfaces getPage failures and resets results when the document disappears', async () => {
    const doc = {
      numPages: 1,
      getPage: vi.fn(async () => { throw new Error('Read failed') }),
    }
    await render(doc, 'mental')
    expect(result.error?.message).toContain('Read failed')
    expect(result.loading).toBe(false)
    await render(null, 'mental')
    expect(result.total).toBe(0)
    expect(result.index).toBe(0)
    expect(result.active).toBeNull()
    expect(result.error).toBeNull()
  })
})
