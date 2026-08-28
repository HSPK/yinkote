import { describe, expect, it, vi } from 'vitest'

import { flatten, loadOutline, pageOf } from './outline'
import type { PDFDocumentProxy } from 'pdfjs-dist'

/** Just enough of a pdf.js document for the parts under test. */
function doc(over: Partial<Record<string, unknown>> = {}): PDFDocumentProxy {
  return {
    getOutline: vi.fn(async () => []),
    getDestination: vi.fn(async () => null),
    // pdf.js counts from zero; everything a reader sees counts from one.
    getPageIndex: vi.fn(async (ref: { num: number }) => ref.num),
    ...over,
  } as unknown as PDFDocumentProxy
}

describe('resolving a bookmark to a page', () => {
  it('follows an explicit destination array', async () => {
    expect(await pageOf(doc(), [{ num: 4 }, { name: 'XYZ' }])).toBe(5)
  })

  it('looks up a named destination first', async () => {
    const d = doc({ getDestination: vi.fn(async () => [{ num: 9 }]) })
    expect(await pageOf(d, 'chapter.3')).toBe(10)
    expect(d.getDestination).toHaveBeenCalledWith('chapter.3')
  })

  it('gives up quietly on a bookmark that points at nothing', async () => {
    // Generated PDFs are full of these. A heading that does nothing is better
    // than one that jumps somewhere wrong.
    expect(await pageOf(doc(), null)).toBeNull()
    expect(await pageOf(doc(), [])).toBeNull()
    expect(await pageOf(doc({ getDestination: vi.fn(async () => null) }), 'missing')).toBeNull()
    const throws = doc({ getPageIndex: vi.fn(async () => { throw new Error('no such ref') }) })
    expect(await pageOf(throws, [{ num: 1 }])).toBeNull()
  })
})

describe('reading a document outline', () => {
  const tree = [
    {
      title: '  Introduction  ',
      dest: [{ num: 0 }],
      items: [
        { title: 'Motivation', dest: [{ num: 1 }], items: [] },
        { title: 'Contributions', dest: [{ num: 2 }], items: [] },
      ],
    },
    { title: 'Method', dest: [{ num: 5 }], items: [] },
  ]

  it('resolves every page and keeps the shape', async () => {
    const nodes = await loadOutline(doc({ getOutline: vi.fn(async () => tree) }))
    expect(nodes).toHaveLength(2)
    expect(nodes[0]).toMatchObject({ title: 'Introduction', page: 1, depth: 0 })
    expect(nodes[0]?.children.map((c) => [c.title, c.page, c.depth])).toEqual([
      ['Motivation', 2, 1],
      ['Contributions', 3, 1],
    ])
    expect(nodes[1]).toMatchObject({ title: 'Method', page: 6, depth: 0 })
  })

  it('resolves destinations together rather than one at a time', async () => {
    // A round trip per heading, in series, down a deep tree is what makes an
    // outline panel feel broken on a thesis.
    let inFlight = 0
    let peak = 0
    const d = doc({
      getOutline: vi.fn(async () => tree),
      getPageIndex: vi.fn(async (ref: { num: number }) => {
        peak = Math.max(peak, ++inFlight)
        await new Promise((r) => setTimeout(r, 1))
        inFlight--
        return ref.num
      }),
    })
    await loadOutline(d)
    expect(peak).toBeGreaterThan(1)
  })

  it('is empty for a document with no outline', async () => {
    // Which the panel shows as absent, not blank.
    expect(await loadOutline(doc({ getOutline: vi.fn(async () => null) }))).toEqual([])
    expect(await loadOutline(doc({ getOutline: vi.fn(async () => []) }))).toEqual([])
    const throws = doc({ getOutline: vi.fn(async () => { throw new Error('encrypted') }) })
    expect(await loadOutline(throws)).toEqual([])
  })

  it('never renders an untitled heading as nothing at all', async () => {
    const nodes = await loadOutline(
      doc({ getOutline: vi.fn(async () => [{ title: '   ', dest: null, items: [] }]) }),
    )
    expect(nodes[0]?.title).toBe('—')
  })
})

describe('flatten', () => {
  it('reads the tree in document order', async () => {
    const nodes = await loadOutline(
      doc({
        getOutline: vi.fn(async () => [
          { title: 'A', dest: null, items: [{ title: 'A.1', dest: null, items: [] }] },
          { title: 'B', dest: null, items: [] },
        ]),
      }),
    )
    expect(flatten(nodes).map((n) => n.title)).toEqual(['A', 'A.1', 'B'])
  })
})
