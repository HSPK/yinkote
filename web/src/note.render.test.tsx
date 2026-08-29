import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { NoteView } from './pages/NoteView'
import { useStore } from './state/store'

let container: HTMLElement
let root: Root

const note = {
  key: 'NOTE1234',
  libraryId: 1,
  itemType: 'note',
  version: 3,
  note: '# Reading plan\n\nStart with section 3.',
  tags: [] as { tag: string; type: number }[],
  collections: [],
  creators: [],
  deleted: false,
  attachments: [],
}

const updates: unknown[] = []

vi.mock('./api/client', () => ({
  api: {
    items: {
      get: () => Promise.resolve(note),
      update: (_lib: number, _key: string, patch: unknown) => {
        updates.push(patch)
        return Promise.resolve(note)
      },
    },
  },
  ApiError: class extends Error {},
}))

beforeEach(() => {
  updates.length = 0
  vi.useFakeTimers()
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({ library: 1 })
})
afterEach(() => {
  act(() => root.unmount())
  container.remove()
  vi.useRealTimers()
})

/** React tracks a controlled input's value, so assigning `.value` and firing
 *  `input` changes nothing: the tracker sees the same value it set. Going
 *  through the prototype's setter is what makes React notice. */
function type_into(element: HTMLTextAreaElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(
    HTMLTextAreaElement.prototype,
    'value',
  )?.set
  setter?.call(element, value)
  element.dispatchEvent(new Event('input', { bubbles: true }))
}

async function show(target?: string) {
  await act(async () => {
    root.render(<NoteView target={target} />)
  })
}

it('opens the note for writing, not for reading', async () => {
  // Clicking a note used to open the PDF reader, which has no PDF to show for
  // one and said "no file" — so a summary was, in practice, unreadable.
  await show('NOTE1234')
  const editor = container.querySelector<HTMLTextAreaElement>('.note-editor')
  expect(editor).not.toBe(null)
  expect(editor?.value).toContain('Reading plan')
})

it('renders the markdown when asked, rather than showing its syntax', async () => {
  await show('NOTE1234')
  const preview = [...container.querySelectorAll('button')].find(
    (b) => b.textContent === 'Preview',
  )
  await act(async () => preview?.click())

  // `#` renders as `h3`: the surface's own headings are h1 and h2, and a
  // note must not compete with them.
  const heading = container.querySelector('.note-preview h3')
  expect(heading?.textContent).toBe('Reading plan')
  expect(container.querySelector('.note-preview')?.textContent).not.toContain('#')
})

it('saves after a pause without being asked', async () => {
  // A note nobody remembered to save is a note that was not written.
  await show('NOTE1234')
  const editor = container.querySelector<HTMLTextAreaElement>('.note-editor')!
  await act(async () => {
    type_into(editor, '# Reading plan\n\nAlso section 4.')
  })
  expect(updates, 'not while still typing').toHaveLength(0)

  await act(async () => {
    vi.advanceTimersByTime(2000)
  })
  // `fields` is nested on a patch, not flattened (3.217) — a flat one answers
  // 200 and changes nothing.
  expect(updates).toEqual([{ fields: { note: '# Reading plan\n\nAlso section 4.' } }])
})

it('does not write the same text back again', async () => {
  await show('NOTE1234')
  await act(async () => {
    vi.advanceTimersByTime(5000)
  })
  expect(updates, 'nothing was changed').toHaveLength(0)
})

it('says so when there is no note to write', async () => {
  await show(undefined)
  expect(container.textContent).toContain('No note selected')
})
