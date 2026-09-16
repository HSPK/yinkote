import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AnnotationTools } from './AnnotationTools'
import type { Annotation } from '../lib/annotations'

const annotation: Annotation = {
  key: 'MARK0001', page: 2, version: 3, space: 'fraction', kind: 'highlight',
  colour: 'amber', text: 'Selected passage', comment: 'Old comment',
  rects: [{ x: 0.1, y: 0.2, w: 0.3, h: 0.04 }],
}
let container: HTMLDivElement
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
    onChange: vi.fn(async () => {}),
    onNote: vi.fn(async () => {}),
    onDelete: vi.fn(async () => {}),
    onDismiss: vi.fn(),
  }
  act(() => root.render(<AnnotationTools annotation={annotation} at={{ x: 200, y: 100 }}
    boxes={[]} {...handlers} />))
  return handlers
}

describe('existing annotation tools', () => {
  it('changes the existing mark colour and creates a note only when asked', async () => {
    const handlers = mount()
    await act(async () => container.querySelector<HTMLButtonElement>('[data-colour="blue"]')?.click())
    expect(handlers.onChange).toHaveBeenCalledWith({ annotationColor: 'blue' })
    expect(handlers.onNote).not.toHaveBeenCalled()
    await act(async () => container.querySelector<HTMLButtonElement>('[aria-label="Create note from annotation"]')?.click())
    expect(handlers.onNote).toHaveBeenCalledTimes(1)
  })

  it('edits and explicitly saves an annotation comment', async () => {
    const handlers = mount()
    await act(async () => container.querySelector<HTMLButtonElement>('[aria-label="Annotation comment"]')?.click())
    const textarea = container.querySelector('textarea')!
    expect(textarea.value).toBe('Old comment')
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(textarea, 'My comment')
      textarea.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect(handlers.onChange).not.toHaveBeenCalled()
    await act(async () => container.querySelector('form')?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })))
    expect(handlers.onChange).toHaveBeenCalledWith({ annotationComment: 'My comment' })
  })

  it('shows save failures without discarding the toolbar', async () => {
    const handlers = mount()
    handlers.onChange.mockRejectedValueOnce(new Error('Save refused'))
    await act(async () => container.querySelector<HTMLButtonElement>('[data-colour="green"]')?.click())
    expect(container.querySelector('[role="alert"]')?.textContent).toContain('Save refused')
    expect(container.querySelector('.annotation-tools')).not.toBeNull()
  })
})
