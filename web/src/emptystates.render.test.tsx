import { afterEach, beforeEach, expect, it } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { ItemTable } from './components/ItemTable'
import { useStore } from './state/store'

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

function show(scope: { query?: string; view?: string; collection?: string | null }) {
  useStore.setState({
    items: [],
    loading: false,
    query: scope.query ?? '',
    view: (scope.view ?? 'library') as never,
    collection: scope.collection ?? null,
    total: 0,
  })
  act(() => root.render(<ItemTable />))
  return container.textContent ?? ''
}

it('tells a new arrival how to get a library in, not how to type one', () => {
  // The first five minutes with the program. "Press a key and create one" is
  // the worst of the three answers: what somebody almost certainly wants is to
  // bring the library they already have.
  const text = show({})
  expect(text).toMatch(/Zotero/)
  expect(text).toMatch(/DOI/)
})

it('does not blame the library when it is a shelf that is empty', () => {
  const text = show({ collection: 'ABCD1234' })
  expect(text).toContain('Nothing here yet')
  expect(text).not.toMatch(/Zotero/)
})

it('says what did not match when there was a search', () => {
  const text = show({ query: 'attention is all you need' })
  expect(text).toContain('attention is all you need')
})
