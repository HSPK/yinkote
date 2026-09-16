import { afterEach, describe, expect, it } from 'vitest'

import { findPdfMatches, indexPdfText } from './pdf-search'
import { pdfSearchRanges } from './pdf-search-ranges'

afterEach(() => {
  window.getSelection()?.removeAllRanges()
  document.body.replaceChildren()
})

describe('pristine PDF search ranges', () => {
  it('maps cross-run hits without splitting text nodes or disturbing selection', () => {
    const host = document.createElement('div')
    host.innerHTML = '<span>men</span><span class="markedContent"><span>tal model</span></span>'
    document.body.append(host)
    const first = host.firstElementChild!.firstChild!
    const last = host.querySelector('.markedContent span')!.firstChild!
    const selection = document.createRange()
    selection.setStart(first, 1)
    selection.setEnd(last, 7)
    window.getSelection()!.addRange(selection)
    const before = host.innerHTML
    const matches = findPdfMatches(indexPdfText([{ str: 'men' }, { str: 'tal model' }]), 'mental', 1)
    for (let pass = 0; pass < 3; pass++) {
      const ranges = pdfSearchRanges(host, matches)
      expect(ranges.map(({ range }) => range.toString())).toEqual(['men', 'tal'])
      expect(ranges[0]!.range.startContainer).toBe(first)
      expect(ranges[1]!.range.endOffset).toBe(3)
      expect(window.getSelection()!.toString()).toBe('ental mod')
      expect(window.getSelection()!.getRangeAt(0)).toBe(selection)
      expect(host.innerHTML).toBe(before)
      expect(host.firstElementChild!.firstChild).toBe(first)
    }
  })

  it('handles UTF-16, ligatures, empty runs, and line break elements', () => {
    const host = document.createElement('div')
    host.innerHTML = '<span>🧠 Oﬃce</span><br><br><span> health</span>'
    const page = indexPdfText([
      { str: '🧠 Oﬃce', hasEOL: true }, { str: '', hasEOL: true }, { str: ' health' },
    ])
    const matches = findPdfMatches(page, 'office health', 1)
    const ranges = pdfSearchRanges(host, matches)
    expect(ranges.map(({ range }) => range.toString())).toEqual(['Oﬃce', ' health'])
    expect(ranges[0]!.range.startOffset).toBe(3)
  })

  it('does not paint truncated or not-yet-rendered text layers', () => {
    const host = document.createElement('div')
    host.innerHTML = '<span>men</span>'
    const matches = findPdfMatches(indexPdfText([{ str: 'mental' }]), 'mental', 1)
    expect(pdfSearchRanges(host, matches)).toEqual([])
    host.replaceChildren()
    expect(pdfSearchRanges(host, matches)).toEqual([])
  })
})
