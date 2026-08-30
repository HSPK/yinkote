import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { copyText } from './clipboard'

/**
 * Copying has to work over plain HTTP.
 *
 * `navigator.clipboard` exists only in a secure context — HTTPS, or localhost.
 * The server binds to the whole machine so a second computer can reach it, and
 * over `http://192.168.x.x` the API is simply absent. Reading `.writeText` off
 * it threw "Cannot read properties of undefined", which the UI reported as
 * "Could not render the citation" — blaming the renderer for a decision the
 * browser had made before it ran.
 */
describe('copying text', () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, 'navigator')

  afterEach(() => {
    if (original) Object.defineProperty(globalThis, 'navigator', original)
    vi.restoreAllMocks()
  })

  beforeEach(() => {
    // jsdom has no execCommand at all.
    Object.defineProperty(document, 'execCommand', {
      configurable: true,
      writable: true,
      value: vi.fn(() => true),
    })
  })

  it('uses the clipboard API when the browser offers one', async () => {
    const writeText = vi.fn(async () => {})
    Object.defineProperty(globalThis, 'navigator', {
      configurable: true,
      value: { clipboard: { writeText } },
    })

    await copyText('a citation')

    expect(writeText).toHaveBeenCalledWith('a citation')
    expect(document.execCommand).not.toHaveBeenCalled()
  })

  it('still copies when there is no clipboard API', async () => {
    // Exactly what a browser hands you over http://192.168.x.x.
    Object.defineProperty(globalThis, 'navigator', {
      configurable: true,
      value: {},
    })

    await expect(copyText('a citation')).resolves.toBeUndefined()
    expect(document.execCommand).toHaveBeenCalledWith('copy')
  })

  it('falls back when the clipboard API refuses', async () => {
    // Permission denied, or the document was not focused. The fallback needs
    // no permission, so it is worth trying before reporting a failure.
    const writeText = vi.fn(async () => {
      throw new Error('NotAllowedError')
    })
    Object.defineProperty(globalThis, 'navigator', {
      configurable: true,
      value: { clipboard: { writeText } },
    })

    await expect(copyText('a citation')).resolves.toBeUndefined()
    expect(document.execCommand).toHaveBeenCalledWith('copy')
  })

  it('leaves nothing behind in the document', async () => {
    Object.defineProperty(globalThis, 'navigator', { configurable: true, value: {} })
    const before = document.body.childElementCount

    await copyText('a citation')

    expect(document.body.childElementCount).toBe(before)
  })
})
