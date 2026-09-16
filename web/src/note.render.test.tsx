import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act, StrictMode } from 'react'
import { readFileSync } from 'node:fs'

import type { Item } from './api/types'
import { NoteView, type NoteViewProps } from './pages/NoteView'
import { useStore } from './state/store'
import { useOverlays } from './ui/overlays'

const api = vi.hoisted(() => ({ get: vi.fn(), update: vi.fn() }))
vi.mock('./api/client', () => ({ api: { items: api }, ApiError: class extends Error {} }))

let container: HTMLElement
let root: Root
let note: Item
let serial = 0
let server: Map<string, Item>

beforeEach(() => {
  vi.useFakeTimers()
  vi.clearAllMocks()
  note = {
    key: `NOTE${String(++serial).padStart(4, '0')}`,
    libraryId: 1, itemType: 'note', version: 3,
    title: 'My reading plan', note: '# Reading plan\n\nStart with section 3.',
    tags: [], collections: [], creators: [], deleted: false,
    dateAdded: 0, dateModified: 0,
  }
  server = new Map([[`1:${note.key}`, note]])
  api.get.mockImplementation((lib: number, key: string) =>
    Promise.resolve(server.get(`${lib}:${key}`) ?? { ...note, key, libraryId: lib }),
  )
  api.update.mockImplementation((lib: number, key: string, patch: { fields: object }) => {
    const stored = server.get(`${lib}:${key}`) ?? { ...note, key, libraryId: lib }
    const updated = { ...stored, ...patch.fields, version: stored.version + 1 }
    server.set(`${lib}:${key}`, updated)
    return Promise.resolve(updated)
  })
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({ library: 1 })
  useOverlays.setState({ toasts: [] })
})

afterEach(async () => {
  await act(async () => root.unmount())
  container.remove()
  vi.clearAllTimers()
  vi.useRealTimers()
})

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

function input(field: 'title' | 'body') {
  return container.querySelector<HTMLInputElement | HTMLTextAreaElement>(
    field === 'title' ? '.note-title-input' : '.note-editor',
  )!
}

async function typeInto(field: 'title' | 'body', value: string) {
  await act(async () => {
    const element = input(field)
    const prototype = field === 'title' ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype
    Object.getOwnPropertyDescriptor(prototype, 'value')!.set!.call(element, value)
    element.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

async function show(props: NoteViewProps = { target: note.key }) {
  await act(async () => root.render(<NoteView {...props} />))
}

async function pause() {
  await act(async () => { vi.advanceTimersByTime(1500) })
}

async function click(text: string) {
  const button = [...container.querySelectorAll('button')].find(
    (b) => b.textContent === text || b.getAttribute('aria-label') === text,
  )
  expect(button, `button ${text}`).toBeDefined()
  await act(async () => button!.click())
}

it('opens a standalone note with a separately editable title and body', async () => {
  await show()
  expect(input('title').value).toBe(note.title)
  expect(input('body').value).toBe(note.note)
  expect(container.querySelector('.note-embedded')).toBeNull()
})

it('uses one Preview switch, rendering safe markdown without a Write mode button', async () => {
  note.note = '# Reading plan\n\n<script>alert(1)</script>\n\n[bad](javascript:alert)'
  await show()
  const switches = container.querySelectorAll<HTMLButtonElement>('[role="switch"]')
  expect(switches).toHaveLength(1)
  const previewSwitch = switches[0]!
  expect(previewSwitch.closest('label')?.textContent).toBe('Preview')
  expect(previewSwitch.getAttribute('aria-checked')).toBe('false')
  expect([...container.querySelectorAll('button')].some((b) => b.textContent === 'Write')).toBe(false)
  await act(async () => previewSwitch.click())
  expect(previewSwitch.getAttribute('aria-checked')).toBe('true')
  expect(container.querySelector('.note-preview h3')?.textContent).toBe('Reading plan')
  expect(container.querySelector('.note-preview script')).toBeNull()
  expect(container.querySelector('.note-preview a')).toBeNull()
  expect(container.querySelector('.note-preview')?.textContent).toContain('<script>alert(1)</script>')
  await act(async () => previewSwitch.click())
  expect(input('body').value).toBe(note.note)
  expect(api.update).not.toHaveBeenCalled()
})

it('saves only fields.title for a title-only edit, using the loaded version', async () => {
  const onSaved = vi.fn()
  await show({ target: note.key, onSaved })
  await typeInto('title', 'A custom title')
  expect(api.update).not.toHaveBeenCalled()
  await pause()
  expect(api.update).toHaveBeenCalledExactlyOnceWith(
    1, note.key, { fields: { title: 'A custom title' } }, 3,
  )
  expect(input('body').value).toBe(note.note)
  expect(onSaved).toHaveBeenCalledWith(expect.objectContaining({ title: 'A custom title' }))
  expect(container.textContent).toContain('Saved')
})

it('saves exact note text after a pause without resending a custom title', async () => {
  await show()
  const body = '  # Changed heading\n\n<literal html>\n\n  '
  await typeInto('body', body)
  expect(api.update).not.toHaveBeenCalled()
  await pause()
  expect(api.update).toHaveBeenCalledExactlyOnceWith(1, note.key, { fields: { note: body } }, 3)
  expect(input('title').value).toBe('My reading plan')
  expect(input('body').value).toBe(body)
})

it('does not write before, during, or after initial load, even in StrictMode', async () => {
  const pending = deferred<Item>()
  api.get.mockReturnValue(pending.promise)
  await act(async () => root.render(<StrictMode><NoteView target={note.key} /></StrictMode>))
  expect(input('body').disabled).toBe(true)
  expect(input('title').disabled).toBe(true)
  expect(input('body').placeholder).toBe('Loading note…')
  await pause()
  expect(api.update).not.toHaveBeenCalled()
  await act(async () => pending.resolve(note))
  await pause()
  expect(input('body').value).toBe(note.note)
  expect(api.update).not.toHaveBeenCalled()
})

it('shows a loading preview rather than an empty note while loading', async () => {
  const pending = deferred<Item>()
  api.get.mockReturnValue(pending.promise)
  await show({ target: note.key, initialPreview: true })
  expect(container.querySelector('.note-preview')?.textContent).toBe('Loading note…')
  expect(container.querySelector('[role="switch"]')?.getAttribute('disabled')).not.toBeNull()
  await act(async () => pending.resolve(note))
  expect(container.querySelector('.note-preview h3')?.textContent).toBe('Reading plan')
  expect(api.update).not.toHaveBeenCalled()
})

it('serializes pending saves and never lets an older response clobber newer text or title', async () => {
  const first = deferred<Item>()
  const second = deferred<Item>()
  api.update.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
  await show()
  await typeInto('body', 'first body')
  await pause()
  await typeInto('body', 'newer body')
  await typeInto('title', 'newer title')
  await pause()
  expect(api.update).toHaveBeenCalledTimes(1)
  await act(async () => first.resolve({ ...note, note: 'first body', version: 4 }))
  expect(input('body').value).toBe('newer body')
  expect(input('title').value).toBe('newer title')
  expect(api.update).toHaveBeenNthCalledWith(
    2, 1, note.key, { fields: { title: 'newer title', note: 'newer body' } }, 4,
  )
  expect(container.textContent).toContain('Saving…')
  await act(async () => second.resolve({
    ...note, title: 'newer title', note: 'newer body', version: 5,
  }))
  expect(input('body').value).toBe('newer body')
  expect(container.textContent).toContain('Saved')
})

it('persists edits reverted to the old value while an earlier save is in flight', async () => {
  const first = deferred<Item>()
  api.update.mockReturnValueOnce(first.promise)
  await show()
  await typeInto('body', 'intermediate')
  await pause()
  await typeInto('body', String(note.note))
  await act(async () => first.resolve({ ...note, note: 'intermediate', version: 4 }))
  expect(api.update).toHaveBeenNthCalledWith(
    2, 1, note.key, { fields: { note: note.note } }, 4,
  )
  expect(input('body').value).toBe(note.note)
})

it('flushes both dirty fields to their original owner when the target changes', async () => {
  const first = deferred<Item>()
  const oldSaved = vi.fn()
  const newSaved = vi.fn()
  api.update.mockReturnValueOnce(first.promise)
  await show({ target: note.key, library: 8, onSaved: oldSaved })
  await typeInto('title', 'old title')
  await typeInto('body', 'old draft')
  await show({ target: 'OTHERKEY', library: 9, onSaved: newSaved })
  expect(api.update).toHaveBeenCalledExactlyOnceWith(
    8, note.key, { fields: { title: 'old title', note: 'old draft' } }, 3,
  )
  expect(api.get).toHaveBeenLastCalledWith(9, 'OTHERKEY')
  await act(async () => first.resolve({
    ...note, libraryId: 8, title: 'old title', note: 'old draft', version: 4,
  }))
  expect(input('body').value).toBe(note.note)
  expect(input('title').value).toBe(note.title)
  expect(oldSaved).toHaveBeenCalledOnce()
  expect(newSaved).not.toHaveBeenCalled()
})

it('isolates identical note keys in different libraries and ignores late loads', async () => {
  const oldLoad = deferred<Item>()
  api.get.mockReturnValueOnce(oldLoad.promise)
  await show({ target: note.key, library: 1 })
  await show({ target: note.key, library: 2 })
  await typeInto('body', 'library two')
  await act(async () => oldLoad.resolve({ ...note, note: 'late library one' }))
  expect(input('body').value).toBe('library two')
  await pause()
  expect(api.update).toHaveBeenCalledExactlyOnceWith(
    2, note.key, { fields: { note: 'library two' } }, 3,
  )
})

it('embeds compactly with initial preview and uses the explicit library, not the store', async () => {
  note.tags = [{ tag: 'summary' }]
  await show({ target: note.key, library: 7, embedded: true, initialPreview: true, onBack: vi.fn() })
  expect(api.get).toHaveBeenCalledExactlyOnceWith(7, note.key)
  expect(container.querySelector('.note-view.note-embedded')).not.toBeNull()
  expect(container.querySelector('.note-preview h3')).not.toBeNull()
  expect(container.querySelector('.note-badge')).toBeNull()
  expect(container.querySelector('[role="tab"]')).toBeNull()
  const back = container.querySelector('button[aria-label="Back"]')
  expect(back?.textContent).toBe('')
  expect(back?.querySelector('svg')).not.toBeNull()
})

it('styles the title left-aligned and lets the preview use the full available width', async () => {
  const style = document.createElement('style')
  const noteStyles = readFileSync('src/pages/note-editor.css', 'utf8')
  style.textContent = `.note-preview { max-width: 78ch; }\n${noteStyles}`
  document.head.append(style)
  try {
    await show({ target: note.key, embedded: true, initialPreview: true })
    const preview = container.querySelector<HTMLElement>('.note-preview')!
    expect(getComputedStyle(preview).maxWidth).toBe('none')
    expect(getComputedStyle(preview).width).toBe('100%')
    expect(getComputedStyle(input('title')).textAlign).toBe('left')
  } finally {
    style.remove()
  }
})

it('waits for pending and newer saves before going Back', async () => {
  const first = deferred<Item>()
  const second = deferred<Item>()
  api.update.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
  const onBack = vi.fn()
  await show({ target: note.key, onBack })
  await typeInto('body', 'first body')
  await pause()
  await typeInto('title', 'last title')
  await click('Back')
  expect(onBack).not.toHaveBeenCalled()
  expect(input('body').disabled).toBe(true)
  await act(async () => first.resolve({ ...note, note: 'first body', version: 4 }))
  expect(onBack).not.toHaveBeenCalled()
  await act(async () => second.resolve({
    ...note, note: 'first body', title: 'last title', version: 5,
  }))
  expect(onBack).toHaveBeenCalledOnce()
})

it('does not run a delayed Back callback after the owner changes', async () => {
  const pending = deferred<Item>()
  api.update.mockReturnValueOnce(pending.promise)
  const onBack = vi.fn()
  await show({ target: note.key, onBack })
  await typeInto('body', 'old draft')
  await click('Back')
  await show({ target: 'NEWOWNER', onBack })
  await act(async () => pending.resolve({ ...note, note: 'old draft', version: 4 }))
  expect(onBack).not.toHaveBeenCalled()
})

it('surfaces save failures, preserves the draft, and keeps Back from leaving until retry succeeds', async () => {
  api.update.mockRejectedValueOnce(new Error('Disk full'))
  const onBack = vi.fn()
  await show({ target: note.key, onBack })
  await typeInto('title', 'Keep me')
  await typeInto('body', 'Keep this too')
  await click('Back')
  expect(onBack).not.toHaveBeenCalled()
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('Disk full')
  expect(input('title').value).toBe('Keep me')
  expect(input('body').value).toBe('Keep this too')
  await pause()
  expect(api.update).toHaveBeenCalledTimes(1)
  await click('Try again')
  expect(container.querySelector('[role="alert"]')).toBeNull()
  await click('Back')
  expect(onBack).toHaveBeenCalledOnce()
})

it('retries loading errors without saving an empty editor over the note', async () => {
  api.get.mockRejectedValueOnce(new Error('Offline'))
  await show()
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('Offline')
  expect(input('body').disabled).toBe(true)
  await pause()
  expect(api.update).not.toHaveBeenCalled()
  await click('Try again')
  expect(input('body').value).toBe(note.note)
  expect(container.querySelector('[role="alert"]')).toBeNull()
  expect(api.update).not.toHaveBeenCalled()
})

it('allows Back after a loading error when there is no draft to lose', async () => {
  api.get.mockRejectedValueOnce(new Error('Note missing'))
  const onBack = vi.fn()
  await show({ target: note.key, onBack })
  await click('Back')
  expect(onBack).toHaveBeenCalledOnce()
  expect(api.update).not.toHaveBeenCalled()
})

it('explicitly resolves version conflicts without replacing untouched remote fields', async () => {
  api.update.mockRejectedValueOnce(Object.assign(new Error('Version changed'), {
    code: 'version_conflict',
  }))
  await show()
  await typeInto('title', 'My title')
  await pause()
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('Someone else changed this first')
  expect(api.update).toHaveBeenCalledTimes(1)
  server.set(`1:${note.key}`, { ...note, title: 'Their title', note: 'Their body', version: 9 })
  await click('Save my changes')
  expect(api.update).toHaveBeenLastCalledWith(
    1, note.key, { fields: { title: 'My title' } }, 9,
  )
  expect(input('body').value).toBe('Their body')
  expect(input('title').value).toBe('My title')
})

it('shares the pending save when a keyed editor is closed and reopened', async () => {
  const pending = deferred<Item>()
  api.update.mockReturnValueOnce(pending.promise)
  await show()
  await typeInto('body', 'first draft')
  await typeInto('title', 'first title')
  await act(async () => root.render(null))
  expect(api.update).toHaveBeenCalledOnce()
  await show()
  expect(input('body').value).toBe('first draft')
  expect(input('title').value).toBe('first title')
  expect(api.get).toHaveBeenCalledOnce()
  await typeInto('body', 'reopened draft')
  await pause()
  expect(api.update).toHaveBeenCalledOnce()
  await act(async () => pending.resolve({
    ...note, note: 'first draft', title: 'first title', version: 4,
  }))
  expect(api.update).toHaveBeenNthCalledWith(
    2, 1, note.key, { fields: { note: 'reopened draft' } }, 4,
  )
  expect(input('body').value).toBe('reopened draft')
})

it('reports a failed close-time save globally and recovers the draft on reopen', async () => {
  const pending = deferred<Item>()
  api.update.mockReturnValueOnce(pending.promise)
  await show()
  await typeInto('body', 'unsaved recovery')
  await act(async () => root.render(null))
  await act(async () => pending.reject(new Error('Offline')))
  expect(useOverlays.getState().toasts).toEqual([
    expect.objectContaining({ tone: 'error', detail: 'Offline' }),
  ])
  await show()
  expect(input('body').value).toBe('unsaved recovery')
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('Offline')
  await click('Try again')
  expect(container.querySelector('[role="alert"]')).toBeNull()
})

it('flushes and warns on browser close only while a draft is unsaved', async () => {
  await show()
  const clean = new Event('beforeunload', { cancelable: true })
  window.dispatchEvent(clean)
  expect(clean.defaultPrevented).toBe(false)
  expect(api.update).not.toHaveBeenCalled()
  await typeInto('title', 'Last-minute title')
  const dirty = new Event('beforeunload', { cancelable: true })
  await act(async () => { window.dispatchEvent(dirty) })
  expect(dirty.defaultPrevented).toBe(true)
  expect(api.update).toHaveBeenCalledExactlyOnceWith(
    1, note.key, { fields: { title: 'Last-minute title' } }, 3,
  )
})

it('says so when there is no note selected', async () => {
  await show({})
  expect(container.textContent).toContain('No note selected')
  expect(api.get).not.toHaveBeenCalled()
  expect(api.update).not.toHaveBeenCalled()
})
