import { act, createRef } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { findPdfMatches, indexPdfText } from '../lib/pdf-search'
import { PdfSearchOverlay } from './PdfSearchOverlay'

let container: HTMLDivElement
let host: HTMLDivElement
let root: Root

beforeEach(() => {
  container = document.createElement('div')
  host = document.createElement('div')
  document.body.append(host, container)
  root = createRoot(container)
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect')
    .mockReturnValue(new DOMRect(100, 200, 600, 800))
  Object.defineProperty(Range.prototype, 'getClientRects', {
    configurable: true,
    value: vi.fn(() => [new DOMRect(130, 240, 60, 16)]),
  })
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
  host.remove()
  vi.restoreAllMocks()
  Reflect.deleteProperty(Range.prototype, 'getClientRects')
})

describe('PDF search overlay', () => {
  it('paints arriving text and active hits outside the selectable text host', async () => {
    const textHost = createRef<HTMLDivElement>()
    Object.defineProperty(textHost, 'current', { value: host })
    const matches = findPdfMatches(indexPdfText([{ str: 'mental mental' }]), 'mental', 1)
    await act(async () => {
      root.render(<PdfSearchOverlay textHost={textHost} matches={matches} activeId={matches[0]!.id} />)
    })
    expect(container.querySelectorAll('.pdf-search-hit')).toHaveLength(0)
    await act(async () => { host.innerHTML = '<span>mental mental</span>' })
    const text = host.firstElementChild!.firstChild
    const marks = container.querySelectorAll<HTMLElement>('.pdf-search-hit')
    expect(marks).toHaveLength(2)
    expect(marks[0]!.dataset.current).toBe('true')
    expect(marks[0]!.style.left).toBe('5%')
    expect(marks[0]!.style.top).toBe('5%')
    expect(marks[0]!.style.width).toBe('10%')
    expect(marks[0]!.style.height).toBe('2%')
    expect(container.querySelector('[aria-hidden="true"]')).not.toBeNull()
    expect(host.querySelector('.pdf-search-hit')).toBeNull()
    await act(async () => {
      root.render(<PdfSearchOverlay textHost={textHost} matches={matches} activeId={matches[1]!.id} zoom={2} />)
    })
    expect(container.querySelectorAll<HTMLElement>('[data-current="true"]')[0]!.dataset.searchMatch)
      .toBe(matches[1]!.id)
    expect(host.firstElementChild!.firstChild).toBe(text)
    await act(async () => { root.render(<PdfSearchOverlay textHost={textHost} />) })
    expect(container.querySelectorAll('.pdf-search-hit')).toHaveLength(0)
    expect(host.textContent).toBe('mental mental')
  })
})
