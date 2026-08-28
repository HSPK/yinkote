import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { Outline } from './Outline'
import type { OutlineNode } from '../lib/outline'

const node = (title: string, page: number | null, depth: number, children: OutlineNode[] = []) =>
  ({ title, page, depth, children }) as OutlineNode

const tree: OutlineNode[] = [
  node('Introduction', 1, 0, [node('Motivation', 2, 1), node('Contributions', 4, 1)]),
  node('Method', 9, 0),
  node('Broken bookmark', null, 0),
]

let container: HTMLElement
let root: Root

beforeEach(() => {
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
})
afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

function mount(current: number, onJump = vi.fn()) {
  act(() => {
    root.render(<Outline nodes={tree} current={current} onJump={onJump} />)
  })
  return onJump
}

describe('the reader outline', () => {
  it('reads the tree in document order, flat', () => {
    mount(1)
    const titles = [...container.querySelectorAll('.outline-title')].map((n) => n.textContent)
    expect(titles).toEqual([
      'Introduction',
      'Motivation',
      'Contributions',
      'Method',
      'Broken bookmark',
    ])
  })

  it('marks the last heading at or before the page being read', () => {
    // Where you are, in words — the one thing on screen that can say it.
    mount(5)
    expect(container.querySelector('[data-active="true"] .outline-title')?.textContent)
      .toBe('Contributions')
    mount(9)
    expect(container.querySelector('[data-active="true"] .outline-title')?.textContent)
      .toBe('Method')
  })

  it('marks nothing before the first heading', () => {
    mount(0)
    expect(container.querySelector('[data-active="true"]')).toBeNull()
  })

  it('jumps to the page a heading names', () => {
    const onJump = mount(1)
    const rows = container.querySelectorAll<HTMLButtonElement>('.outline-row')
    act(() => rows[3]?.click())
    expect(onJump).toHaveBeenCalledWith(9)
  })

  it('does not pretend a broken bookmark is a link', () => {
    const onJump = mount(1)
    const rows = container.querySelectorAll<HTMLButtonElement>('.outline-row')
    const broken = rows[4]
    expect(broken?.disabled).toBe(true)
    act(() => broken?.click())
    expect(onJump).not.toHaveBeenCalled()
    // The row stays, because it still says what is in the document — it just
    // has no page number beside it.
    expect(broken?.querySelector('.outline-page')).toBeNull()
  })

  it('indents by depth, capped so deep headings still read', () => {
    mount(1)
    const rows = container.querySelectorAll<HTMLElement>('.outline-row')
    expect(rows[0]?.style.paddingLeft).toBe('6px')
    expect(rows[1]?.style.paddingLeft).toBe('16px')
  })
})
