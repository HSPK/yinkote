/**
 * Watching a long job.
 *
 * The parts worth pinning down are the ones a polling loop gets wrong: not
 * stopping, and inventing a percentage it does not know.
 */
import { describe, expect, it, vi } from 'vitest'

import { percentOf } from './tasks'
import type { Task } from '../api/types'

const task = (over: Partial<Task> = {}): Task => ({
  id: 't1',
  kind: 'import',
  phase: 'running',
  message: 'Reading',
  done: 0,
  total: 0,
  startedAt: 0,
  ...over,
})

describe('percentOf', () => {
  it('reports a fraction when the job knows one', () => {
    expect(percentOf(task({ done: 30, total: 120 }))).toBe(25)
  })

  it('says nothing rather than guessing', () => {
    // A bar that invents a percentage is worse than a spinner: it is believed.
    expect(percentOf(task({ done: 40, total: 0 }))).toBeNull()
  })

  it('never claims more than finished', () => {
    // Totals are estimates and can be overtaken.
    expect(percentOf(task({ done: 200, total: 120 }))).toBe(100)
  })
})

describe('follow', () => {
  it('stops when the job stops, and returns how it ended', async () => {
    vi.resetModules()
    const states: Task[] = [
      task({ phase: 'running', done: 1, total: 3 }),
      task({ phase: 'running', done: 2, total: 3 }),
      task({ phase: 'failed', error: 'the disk is full' }),
    ]
    let i = 0
    vi.doMock('../api/client', () => ({
      api: { tasks: { get: () => Promise.resolve(states[Math.min(i++, states.length - 1)]) } },
    }))
    const { follow } = await import('./tasks')

    const seen: number[] = []
    const done = await follow('t1', (t) => seen.push(t.done))

    // A failure is a result, not an exception: the caller wants to say what
    // went wrong, and a rejected promise makes that harder.
    expect(done?.phase).toBe('failed')
    expect(done?.error).toBe('the disk is full')
    expect(seen).toEqual([1, 2, 0])
  }, 10_000)

  it('gives up on a job the server has forgotten', async () => {
    vi.resetModules()
    vi.doMock('../api/client', () => ({
      api: { tasks: { get: () => Promise.reject(new Error('404')) } },
    }))
    const { follow } = await import('./tasks')
    // Finished long enough ago to be pruned. There is nothing to report and
    // nothing to wait for, so the loop must not spin forever.
    expect(await follow('t1')).toBeNull()
  })
})
