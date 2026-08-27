/**
 * The long-running maintenance jobs.
 *
 * Rebuilding the search index takes about thirty seconds on a hundred
 * thousand items. The thing worth testing is that it says so — a button that
 * is silent for half a minute reads as a button that did not work.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { runOptimize, runReindex } from './maintenance'
import { useStore } from '../state/store'
import { useOverlays } from '../ui/overlays'

beforeEach(() => {
  useOverlays.setState({ toasts: [] })
})

describe('maintenance jobs', () => {
  it('says the index is being rebuilt while it is', async () => {
    let finish: () => void = () => {}
    useStore.setState({ reindex: () => new Promise<void>((r) => (finish = r)) } as never)

    const running = runReindex()
    expect(useOverlays.getState().toasts.map((t) => t.message)).toContain(
      'Rebuilding the search index…',
    )

    finish()
    await running
    expect(useOverlays.getState().toasts.map((t) => t.message)).not.toContain(
      'Rebuilding the search index…',
    )
  })

  it('takes the message down when the job fails', async () => {
    useStore.setState({ reindex: () => Promise.reject(new Error('nope')) } as never)
    await runReindex()

    // A "rebuilding…" left up after a failure says the thing is still coming.
    const messages = useOverlays.getState().toasts.map((t) => t.message)
    expect(messages).not.toContain('Rebuilding the search index…')
  })

  it('reports the same way for compaction', async () => {
    const done = vi.fn().mockResolvedValue(undefined)
    useStore.setState({ optimize: done } as never)
    await runOptimize()

    expect(done).toHaveBeenCalled()
    expect(useOverlays.getState().toasts.map((t) => t.message)).toContain('Database optimised')
  })
})
