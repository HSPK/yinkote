import { afterEach, beforeEach, expect, it } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { BrowserConnector, LibraryAccess } from './BrowserConnector'
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

it('says browser saving is off, and offers to turn it on here', () => {
  // The whole reason this component exists: the feature was off and said so
  // nowhere, so "off" has to be legible and actionable rather than absent.
  //
  // "Actionable" used to mean naming the `--connector-port` flag, which is
  // unreachable on a service install — the advice was real and the user could
  // not follow it. The switch is on this page now, so the flag is no longer
  // the answer and this asserts the answer that is.
  const text = mount({ state: 'off' }).textContent ?? ''
  expect(text).toContain('Off')
  expect(text).not.toContain('--connector-port')
  expect([...container.querySelectorAll('button')].map((b) => b.textContent)).toContain('Turn on')
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

it('says plainly when anyone can reach the library', () => {
  // The state that carries the risk looked exactly like the one that does
  // not: a server past loopback with no key said nothing at startup and
  // nothing in /ping, so a choice made once in a service file was never
  // shown again.
  act(() => root.render(<LibraryAccess access={{ state: 'open' }} />))
  const text = container.textContent ?? ''
  expect(text).toContain('Reachable by anyone')
  expect(text).toMatch(/read, edit and delete/)
  expect(container.querySelector('.badge')?.getAttribute('data-tone')).toBe('warn')
})

it('does not cry wolf about the default', () => {
  act(() => root.render(<LibraryAccess access={{ state: 'private' }} />))
  expect(container.querySelector('.badge')?.getAttribute('data-tone')).toBe('ok')
  expect(container.textContent ?? '').toContain('This machine only')
})
