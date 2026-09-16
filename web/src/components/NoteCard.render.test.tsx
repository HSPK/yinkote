import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Item } from '../api/types'
import { NoteCard } from './NoteCard'
import { notePreview } from '../lib/note-preview'

const note: Item = {
  key: 'NOTE0001', itemType: 'note', libraryId: 1, version: 1, deleted: false,
  creators: [], collections: [], tags: [{ tag: 'summary', type: 1 }],
  note: '<p>## A **summary** &amp; useful details</p>',
  title: 'Summary',
  dateAdded: 1, dateModified: 1788715200000,
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

describe('shared note rows', () => {
  it('uses the same card, metadata and excerpt structure as marks', () => {
    const open = vi.fn()
    act(() => root.render(<NoteCard note={note} onOpen={open} />))
    const row = container.querySelector<HTMLButtonElement>('.note-card')!
    expect(row.dataset.colour).toBe('blue')
    expect(row.querySelector('.note-page')?.textContent).toContain('Note')
    expect(row.querySelector('.note-card-title')?.textContent).toBe('Summary')
    expect(row.querySelector('.note-comment, .note-badge')).toBeNull()
    expect(row.querySelector('.note-text')?.textContent).toBe('A summary & useful details')
    expect(row.querySelector('time')?.dateTime).toBe(new Date(note.dateModified).toISOString())
    act(() => row.click())
    expect(open).toHaveBeenCalledTimes(1)
  })

  it('keeps previews bounded and strips presentation markup without rendering HTML', () => {
    expect(notePreview({ ...note, note: 'x'.repeat(10000) })).toHaveLength(500)
    expect(notePreview({ ...note, note: '[A title](https://example.com)\n> quoted' }))
      .toBe('A title quoted')
  })
})
