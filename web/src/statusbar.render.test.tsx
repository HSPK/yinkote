import { afterEach, beforeEach, expect, it } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { StatusBar } from './components/StatusBar'
import { ApiError } from './api/client'
import { useI18n } from './i18n'
import { failureOf } from './lib/errors'
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
  useI18n.setState({ locale: 'en-US' })
  useStore.setState({ error: null })
})

function show(locale: 'en-US' | 'zh-CN', error: unknown) {
  useI18n.setState({ locale })
  useStore.setState({ error: error === null ? null : failureOf(error) })
  act(() => root.render(<StatusBar />))
  return container.querySelector('.err')
}

it('says what went wrong in the language being read', () => {
  // The store used to hold the server's English sentence, so this bar showed
  // it whatever the reader had chosen.
  const failed = new ApiError(503, 'unavailable', 'unavailable: the search index is rebuilding')
  expect(show('zh-CN', failed)?.textContent).toContain('该服务未响应')
  expect(show('en-US', failed)?.textContent).toContain('not answering')
})

it('keeps the server’s own words within reach', () => {
  // The class says what kind of failure; only the detail says which thing.
  const failed = new ApiError(404, 'not_found', 'not found: no such collection')
  expect(show('en-US', failed)?.getAttribute('title')).toBe('not found: no such collection')
})

it('shows a failure it cannot classify rather than nothing', () => {
  // A server that is not running produces a browser string and no code.
  expect(show('en-US', new TypeError('Failed to fetch'))?.textContent).toContain('Failed to fetch')
})

it('shows no warning when nothing has gone wrong', () => {
  expect(show('en-US', null)).toBe(null)
})
