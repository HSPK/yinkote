import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { PageRail } from './PageRail'

/**
 * jsdom has no IntersectionObserver, and the rail is built around one: nothing
 * is drawn until a cell comes near the viewport, because every miss is a full
 * pdf.js page render.
 */
class FakeObserver {
  static instances: FakeObserver[] = []
  targets: Element[] = []
  constructor(_cb: IntersectionObserverCallback) {
    FakeObserver.instances.push(this)
  }
  observe(el: Element) {
    this.targets.push(el)
  }
  disconnect() {}
}

let container: HTMLElement
let root: Root

beforeEach(() => {
  FakeObserver.instances = []
  vi.stubGlobal('IntersectionObserver', FakeObserver)
  Element.prototype.scrollIntoView = vi.fn()
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
  vi.unstubAllGlobals()
})

function mount(current: number, onJump = vi.fn()) {
  act(() => {
    root.render(
      <PageRail
        library={1}
        attachmentKey="ABCD1234"
        pages={[1, 2, 3, 4, 5]}
        current={current}
        onJump={onJump}
      />,
    )
  })
  return onJump
}

describe('the reader page rail', () => {
  it('offers a cell per page and marks the one being read', () => {
    mount(3)
    expect(container.querySelectorAll('[data-page]')).toHaveLength(5)
    expect(container.querySelector('[data-active="true"]')?.getAttribute('data-page')).toBe('3')
  })

  it('draws nothing until a cell comes near the viewport', () => {
    mount(1)
    // A rail that drew every page would render a whole thesis before showing
    // page one.
    expect(container.querySelectorAll('img')).toHaveLength(0)
    expect(FakeObserver.instances[0]?.targets).toHaveLength(5)
  })

  it('jumps to the page that was clicked', () => {
    const onJump = mount(1)
    act(() => {
      ;(container.querySelector('[data-page="4"]') as HTMLElement).click()
    })
    expect(onJump).toHaveBeenCalledWith(4)
  })

  it('keeps the page being read in view', () => {
    mount(2)
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled()
  })

  it('shows every page number', () => {
    mount(1)
    const numbers = [...container.querySelectorAll('.page-number')].map((n) => n.textContent)
    expect(numbers).toEqual(['1', '2', '3', '4', '5'])
  })
})
