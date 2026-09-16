import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { pdfSelection } from './pdf-selection'

let root: HTMLElement
let first: Text
let second: Text

beforeEach(() => {
  root = document.createElement('div')
  for (const [index, text] of ['first page words', 'second page words'].entries()) {
    const page = document.createElement('div')
    page.className = 'pdf-page'
    page.dataset.page = String(index + 1)
    const layer = document.createElement('div')
    layer.className = 'pdf-text'
    const span = document.createElement('span')
    span.textContent = text
    layer.append(span)
    page.append(layer)
    root.append(page)
  }
  document.body.append(root)
  first = root.querySelectorAll('span')[0]!.firstChild as Text
  second = root.querySelectorAll('span')[1]!.firstChild as Text
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    const number = Number(this.dataset.page)
    return new DOMRect(0, (number - 1) * 220, 200, 200)
  })
  Object.defineProperty(Range.prototype, 'getClientRects', {
    configurable: true,
    value: function (this: Range) {
      const number = Number(this.startContainer.parentElement?.closest('.pdf-page')?.getAttribute('data-page'))
      return [new DOMRect(10, (number - 1) * 220 + 20, 100, 10)]
    },
  })
})

afterEach(() => {
  window.getSelection()?.removeAllRanges()
  root.remove()
  Reflect.deleteProperty(Range.prototype, 'getClientRects')
  vi.restoreAllMocks()
})

describe('PDF selection ownership', () => {
  it('does not insert spaces inside words split by find-in-document marks', () => {
    const span = root.querySelector('span')!
    const mark = document.createElement('span')
    mark.dataset.found = 'true'
    mark.textContent = 'light'
    span.replaceChildren(document.createTextNode('high'), mark, document.createTextNode('ing'))
    const range = document.createRange()
    range.selectNodeContents(span)
    const selection = window.getSelection()!
    selection.addRange(range)
    expect(pdfSelection(root, selection)[0]?.text).toBe('highlighting')
  })

  it('stores separate text and page-relative geometry for a cross-page selection', () => {
    const range = document.createRange()
    range.setStart(first, 6)
    range.setEnd(second, 6)
    const selection = window.getSelection()!
    selection.addRange(range)
    expect(pdfSelection(root, selection)).toEqual([
      { page: 1, text: 'page words', rects: [{ x: 0.05, y: 0.1, w: 0.5, h: 0.05 }] },
      { page: 2, text: 'second', rects: [{ x: 0.05, y: 0.1, w: 0.5, h: 0.05 }] },
    ])
  })

  it('does not annotate a selection in the notes panel or another document', () => {
    const outside = document.createElement('p')
    outside.textContent = 'outside'
    document.body.append(outside)
    const range = document.createRange()
    range.setStart(first, 1)
    range.setEnd(outside.firstChild!, 3)
    const selection = window.getSelection()!
    selection.addRange(range)
    expect(pdfSelection(root, selection)).toEqual([])
    outside.remove()
  })

  it('never creates a mark from a caret', () => {
    const selection = window.getSelection()!
    selection.collapse(first, 4)
    expect(pdfSelection(root, selection)).toEqual([])
  })
})
