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

describe('withToast while something is running', () => {
  it('says so while it waits, and stops saying so when it finishes', async () => {
    useOverlays.setState({ toasts: [] })

    let finish: (v: number) => void = () => {}
    const running = withToast(() => new Promise<number>((r) => (finish = r)), {
      pending: 'Summarising…',
      success: 'Done',
      failure: 'Failed',
    })

    // Some of these are model calls that run for most of a minute. With only
    // an outcome message the interface says nothing at all while they run,
    // and the honest reading of that is "the click did not work".
    expect(useOverlays.getState().toasts.map((t) => t.message)).toContain('Summarising…')

    finish(1)
    await running
    expect(useOverlays.getState().toasts.map((t) => t.message)).not.toContain('Summarising…')
    expect(useOverlays.getState().toasts.map((t) => t.message)).toContain('Done')
  })

  it('takes the waiting message down when it fails too', async () => {
    useOverlays.setState({ toasts: [] })

    await withToast(() => Promise.reject(new Error('nope')), {
      pending: 'Summarising…',
      failure: 'Failed',
    })

    // A "working…" left on screen after a failure is worse than none: it says
    // the thing is still coming.
    const messages = useOverlays.getState().toasts.map((t) => t.message)
    expect(messages).not.toContain('Summarising…')
    expect(messages).toContain('Failed')
  })

  it('says nothing extra when no waiting message was given', async () => {
    useOverlays.setState({ toasts: [] })
    await withToast(async () => 1, { success: 'Done', failure: 'Failed' })
    expect(useOverlays.getState().toasts).toHaveLength(1)
  })
})
