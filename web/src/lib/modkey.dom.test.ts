import { afterEach, expect, it } from 'vitest'

import { modKey } from './format'

const original = Object.getOwnPropertyDescriptor(window.navigator, 'platform')
const pretend = (value: string) =>
  Object.defineProperty(window.navigator, 'platform', { value, configurable: true })

afterEach(() => {
  if (original) Object.defineProperty(window.navigator, 'platform', original)
})

it('names the key the machine in front of you actually has', () => {
  // The handlers accept metaKey *or* ctrlKey, so both platforms worked; only
  // the label was wrong, and it was wrong for everyone not on a Mac — in a
  // program whose premise is being cross-platform.
  pretend('MacIntel')
  expect(modKey()).toBe('⌘')
  pretend('Linux x86_64')
  expect(modKey()).toBe('Ctrl+')
  pretend('Win32')
  expect(modKey()).toBe('Ctrl+')
})
