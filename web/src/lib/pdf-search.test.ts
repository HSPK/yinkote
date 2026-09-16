import { describe, expect, it } from 'vitest'

import { findPdfMatches, indexPdfText, pdfSearchQuery } from './pdf-search'

describe('PDF text matching', () => {
  it('finds mental across PDF text runs, not just within one DOM text node', () => {
    const page = indexPdfText([
      { str: 'experi' }, { type: 'beginMarkedContent' },
      { str: 'men' }, { str: 'tal and MENTAL' },
    ])
    const matches = findPdfMatches(page, 'mental', 7)
    expect(matches).toHaveLength(2)
    expect(matches[0]).toEqual({
      id: '7:6:12', pageNumber: 7,
      start: { run: 1, offset: 0, textOffset: 6 },
      end: { run: 2, offset: 3, textOffset: 12 },
    })
    expect(matches[1]!.start).toEqual({ run: 2, offset: 8, textOffset: 17 })
  })

  it('normalises ligatures while returning original UTF-16 offsets', () => {
    const page = indexPdfText([{ str: '🧠 Oﬃce mental' }])
    expect(findPdfMatches(page, 'OFFICE', 1)[0]).toMatchObject({
      start: { offset: 3, textOffset: 3 },
      end: { offset: 7, textOffset: 7 },
    })
    expect(findPdfMatches(page, 'mental', 1)[0]!.start.offset).toBe(8)
    expect(findPdfMatches(page, '🧠', 1)[0]!.end.offset).toBe(2)
    expect(findPdfMatches(indexPdfText([{ str: 'ﬀ' }]), 'f', 1)).toHaveLength(1)
  })

  it('finds phrases through whitespace, EOLs, and empty text runs', () => {
    const page = indexPdfText([
      { str: 'mental', hasEOL: true }, { str: '', hasEOL: true },
      { str: '\u00a0  health\tmodel' },
    ])
    expect(page.text).toBe('mental health model')
    const matches = findPdfMatches(page, '  mental\n health  ', 1)
    expect(matches).toHaveLength(1)
    expect(matches[0]!.start).toEqual({ run: 0, offset: 0, textOffset: 0 })
    expect(matches[0]!.end).toEqual({ run: 2, offset: 9, textOffset: 15 })
    expect(findPdfMatches(page, 'mentalhealth', 1)).toHaveLength(0)
  })

  it('maps combining sequences crossing text runs and ignored soft hyphens', () => {
    const page = indexPdfText([{ str: 'Caf' }, { str: 'e' }, { str: '\u0301 men\u00adtal' }])
    expect(findPdfMatches(page, 'café', 1)[0]!.end).toEqual({
      run: 2, offset: 1, textOffset: 5,
    })
    const mental = findPdfMatches(page, 'mental', 1)[0]!
    expect(mental.end.textOffset - mental.start.textOffset).toBe(7)
  })

  it('ignores blank queries and non-overlapping duplicates', () => {
    const page = indexPdfText([{ str: 'aaaa' }])
    expect(findPdfMatches(page, ' \n ', 1)).toEqual([])
    expect(findPdfMatches(page, 'aa', 1)).toHaveLength(2)
    expect(pdfSearchQuery('\u00a0  MENTAL\tModel ')).toBe('mental model')
  })
})
