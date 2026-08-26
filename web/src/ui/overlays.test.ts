import { afterEach, describe, expect, it, vi } from 'vitest'

import { confirmAction, promptFor, toast, useOverlays, withToast } from './overlays'

const reset = () => useOverlays.setState({ dialog: null, menu: null, toasts: [] })

afterEach(() => {
  reset()
  vi.useRealTimers()
})

describe('dialogs', () => {
  it('resolves with the submitted values', async () => {
    const pending = useOverlays.getState().ask({ title: 'T', fields: [{ name: 'a', label: 'A' }] })
    expect(useOverlays.getState().dialog?.title).toBe('T')

    useOverlays.getState().resolveDialog({ a: '1' })
    await expect(pending).resolves.toEqual({ a: '1' })
    expect(useOverlays.getState().dialog).toBeNull()
  })

  it('resolves with null when cancelled', async () => {
    const pending = useOverlays.getState().ask({ title: 'T' })
    useOverlays.getState().resolveDialog(null)
    await expect(pending).resolves.toBeNull()
  })

  it('cancels a dialog that a second one replaces, rather than stacking', async () => {
    const first = useOverlays.getState().ask({ title: 'first' })
    const second = useOverlays.getState().ask({ title: 'second' })

    await expect(first).resolves.toBeNull()
    expect(useOverlays.getState().dialog?.title).toBe('second')

    useOverlays.getState().resolveDialog({})
    await expect(second).resolves.toEqual({})
  })

  it('ignores a resolve when nothing is open', () => {
    expect(() => useOverlays.getState().resolveDialog({})).not.toThrow()
  })
})

describe('promptFor', () => {
  it('returns the trimmed value', async () => {
    const pending = promptFor('Title', { label: 'Name' })
    useOverlays.getState().resolveDialog({ value: '  spaced  ' })
    await expect(pending).resolves.toBe('spaced')
  })

  it('marks the single field required and focused', async () => {
    const pending = promptFor('Title', { label: 'Name' })
    const field = useOverlays.getState().dialog?.fields?.[0]
    expect(field?.required).toBe(true)
    expect(field?.autoFocus).toBe(true)
    useOverlays.getState().resolveDialog(null)
    await pending
  })

  it('returns null when cancelled', async () => {
    const pending = promptFor('Title', { label: 'Name' })
    useOverlays.getState().resolveDialog(null)
    await expect(pending).resolves.toBeNull()
  })
})

describe('confirmAction', () => {
  it('is true only when confirmed', async () => {
    const yes = confirmAction('Sure?')
    useOverlays.getState().resolveDialog({})
    await expect(yes).resolves.toBe(true)

    const no = confirmAction('Sure?')
    useOverlays.getState().resolveDialog(null)
    await expect(no).resolves.toBe(false)
  })
})

describe('context menu', () => {
  it('opens at the given position', () => {
    useOverlays.getState().openMenu(10, 20, [{ label: 'x' }])
    expect(useOverlays.getState().menu).toMatchObject({ x: 10, y: 20 })
  })

  it('refuses to open an empty menu', () => {
    useOverlays.getState().openMenu(10, 20, [])
    expect(useOverlays.getState().menu).toBeNull()
  })

  it('closes', () => {
    useOverlays.getState().openMenu(1, 1, [{ label: 'x' }])
    useOverlays.getState().closeMenu()
    expect(useOverlays.getState().menu).toBeNull()
  })
})

describe('toasts', () => {
  it('stacks and dismisses individually', () => {
    const a = toast.info('one')
    const b = toast.error('two')
    expect(useOverlays.getState().toasts).toHaveLength(2)

    useOverlays.getState().dismissToast(a)
    expect(useOverlays.getState().toasts.map((t) => t.id)).toEqual([b])
  })

  it('auto-dismisses after its tone-specific delay', () => {
    vi.useFakeTimers()
    toast.success('done')
    expect(useOverlays.getState().toasts).toHaveLength(1)
    vi.advanceTimersByTime(2600)
    expect(useOverlays.getState().toasts).toHaveLength(0)
  })

  it('keeps errors up longer than successes', () => {
    vi.useFakeTimers()
    toast.error('bad')
    vi.advanceTimersByTime(3000)
    expect(useOverlays.getState().toasts).toHaveLength(1)
    vi.advanceTimersByTime(5000)
    expect(useOverlays.getState().toasts).toHaveLength(0)
  })

  it('formats a caught error into the detail line', () => {
    toast.fromError('Failed', new Error('boom'))
    expect(useOverlays.getState().toasts[0]).toMatchObject({
      tone: 'error',
      message: 'Failed',
      detail: 'boom',
    })
  })

  it('stringifies non-Error throws', () => {
    toast.fromError('Failed', 'plain string')
    expect(useOverlays.getState().toasts[0]?.detail).toBe('plain string')
  })
})

describe('withToast', () => {
  it('returns the value and reports success', async () => {
    const result = await withToast(async () => 42, { success: 'ok', failure: 'no' })
    expect(result).toBe(42)
    expect(useOverlays.getState().toasts[0]).toMatchObject({ tone: 'success', message: 'ok' })
  })

  it('swallows the failure and reports it, so a click never dies silently', async () => {
    const result = await withToast(
      async () => {
        throw new Error('nope')
      },
      { failure: 'could not save' },
    )
    expect(result).toBeUndefined()
    expect(useOverlays.getState().toasts[0]).toMatchObject({
      tone: 'error',
      message: 'could not save',
      detail: 'nope',
    })
  })

  it('stays quiet on success when no message is given', async () => {
    await withToast(async () => 1, { failure: 'x' })
    expect(useOverlays.getState().toasts).toHaveLength(0)
  })
})
