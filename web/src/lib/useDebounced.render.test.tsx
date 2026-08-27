/**
 * A value that lags behind.
 *
 * The point is what does *not* happen: the values passed through on the way
 * to the one that settles.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import { useDebounced } from './useDebounced'

let root: Root | null = null
let container: HTMLElement | null = null

function render(initial: string) {
  const seen: string[] = []
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)

  function Probe({ value }: { value: string }) {
    const settled = useDebounced(value, 50)
    seen.push(settled)
    return null
  }

  const show = (value: string) =>
    act(() => {
      root!.render(<Probe value={value} />)
    })

  void show(initial)
  return { seen, show }
}

afterEach(() => {
  if (root) act(() => root!.unmount())
  container?.remove()
  root = null
  vi.useRealTimers()
})

describe('a debounced value', () => {
  it('shows the first value at once', () => {
    // Waiting to show anything at all would make opening a paper feel slower
    // than it is.
    const { seen } = render('a')
    expect(seen[0]).toBe('a')
  })

  it('skips the values passed through', async () => {
    vi.useFakeTimers()
    const { seen, show } = render('a')

    await show('b')
    await show('c')
    await show('d')
    // Nothing settled yet: b and c were on the way to d.
    expect(seen).not.toContain('b')
    expect(seen).not.toContain('c')

    await act(async () => {
      vi.advanceTimersByTime(60)
    })
    expect(seen.at(-1)).toBe('d')
    expect(seen).not.toContain('b')
  })

  it('settles on the last value even when it arrives late', async () => {
    vi.useFakeTimers()
    const { seen, show } = render('a')

    await show('b')
    await act(async () => {
      vi.advanceTimersByTime(60)
    })
    expect(seen.at(-1)).toBe('b')
  })
})
