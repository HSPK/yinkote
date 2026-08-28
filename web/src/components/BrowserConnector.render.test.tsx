import { afterEach, beforeEach, expect, it } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { BrowserConnector } from './BrowserConnector'
import type { ConnectorStatus } from '../api/types'

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

function mount(status?: ConnectorStatus) {
  act(() => {
    root.render(<BrowserConnector status={status} />)
  })
  return container
}

it('says browser saving is off, and how to turn it on', () => {
  // The whole reason this component exists: the feature was off and said so
  // nowhere, so "off" has to be legible and actionable rather than absent.
  const text = mount({ state: 'off' }).textContent ?? ''
  expect(text).toContain('Off')
  expect(text).toContain('--connector-port')
  // No port to show when none was ever asked for.
  expect(container.querySelector('code')).toBeNull()
})

it('names the port it is listening on', () => {
  const text = mount({ state: 'listening', port: 23119 }).textContent ?? ''
  expect(text).toContain('Listening')
  expect(text).toContain('23119')
  expect(container.querySelector('.badge')?.getAttribute('data-tone')).toBe('ok')
})

it('warns when the port was asked for and refused', () => {
  // The case that would otherwise look identical to working: requested, but
  // something else — nearly always a running Zotero — already has the port.
  const text = mount({ state: 'unavailable', port: 23119 }).textContent ?? ''
  expect(text).toContain('23119')
  expect(text).toMatch(/Zotero/)
  expect(container.querySelector('.badge')?.getAttribute('data-tone')).toBe('warn')
})

it('waits rather than guessing before the server has answered', () => {
  // Reporting "off" while /ping is still in flight would be a wrong answer
  // that looks exactly like a right one.
  const text = mount(undefined).textContent ?? ''
  expect(text).not.toContain('Off')
})
