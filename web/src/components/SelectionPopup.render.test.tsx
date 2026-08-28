import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { SelectionPopup } from './SelectionPopup'
import { HIGHLIGHT_COLOURS } from '../lib/annotations'

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

function mount() {
  const handlers = {
    onMark: vi.fn(),
    onCopy: vi.fn(),
    onCite: vi.fn(),
    onDismiss: vi.fn(),
  }
  act(() => {
    root.render(<SelectionPopup at={{ x: 100, y: 50 }} colour="amber" {...handlers} />)
  })
  return handlers
}

describe('the selection popup', () => {
  it('writes nothing until something is chosen', () => {
    // The whole point: releasing the mouse over a selection used to create a
    // highlight, so reading with the mouse edited the library.
    const h = mount()
    expect(h.onMark).not.toHaveBeenCalled()
    expect(h.onCopy).not.toHaveBeenCalled()
    expect(h.onCite).not.toHaveBeenCalled()
  })

  it('offers every colour and marks the one in use', () => {
    mount()
    const swatches = container.querySelectorAll('.swatch')
    expect(swatches).toHaveLength(HIGHLIGHT_COLOURS.length)
    expect(container.querySelector('.swatch[data-active="true"]')?.getAttribute('data-colour'))
      .toBe('amber')
  })

  it('highlights in the colour that was clicked, not the current one', () => {
    const h = mount()
    const green = container.querySelector<HTMLElement>('.swatch[data-colour="green"]')
    act(() => green?.click())
    expect(h.onMark).toHaveBeenCalledWith('highlight', 'green')
  })

  it('underlines in the current colour', () => {
    const h = mount()
    const actions = container.querySelectorAll<HTMLButtonElement>('.popup-action')
    act(() => actions[0]?.click())
    expect(h.onMark).toHaveBeenCalledWith('underline', 'amber')
  })

  it('offers copying the text and copying it with a citation', () => {
    const h = mount()
    const actions = container.querySelectorAll<HTMLButtonElement>('.popup-action')
    act(() => actions[1]?.click())
    expect(h.onCopy).toHaveBeenCalled()
    act(() => actions[2]?.click())
    expect(h.onCite).toHaveBeenCalled()
  })

  it('closes on Escape', () => {
    const h = mount()
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    })
    expect(h.onDismiss).toHaveBeenCalled()
  })
})
