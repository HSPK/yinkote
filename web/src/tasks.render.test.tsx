/**
 * Jobs, rendered.
 *
 * The interesting column is the outcome: each kind of job answers with its own
 * shape, and the useful part differs — an export is a file somebody has to
 * find, an import is a count of what arrived. That is the part that will break
 * when a job's result changes, so that is what is pinned here.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Task } from './api/types'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

const cancelled: string[] = []
let tasks: Task[] = []

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: (_t, _this, args: unknown[]) => {
        if (path === 'api.tasks.list') return Promise.resolve({ tasks })
        if (path === 'api.tasks.cancel') {
          cancelled.push(args[0] as string)
          return Promise.resolve({ cancelled: true })
        }
        return new Promise(() => {})
      },
    })
  return { api: build('api'), connectEvents: () => () => {} }
})

beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as never
  Element.prototype.getBoundingClientRect = () =>
    ({ width: 1200, height: 800, top: 0, left: 0, right: 1200, bottom: 800, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 800 })
})

let container: HTMLElement
let root: Root

const task = (over: Partial<Task> = {}): Task => ({
  id: 't1',
  kind: 'export',
  phase: 'done',
  message: 'Packing',
  done: 0,
  total: 0,
  startedAt: 1_700_000_000,
  ...over,
})

beforeEach(() => {
  cancelled.length = 0
  tasks = []
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [{ id: 'tasks', kind: 'tasks', title: 'Jobs' }],
    activeTab: 'tasks',
    scopes: {},
    collections: [],
    smartCollections: [],
    tags: [],
    badgeDefs: [],
  })
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

async function render() {
  await act(async () => {
    root.render(<App />)
  })
  await act(async () => {
    await Promise.resolve()
  })
}

const rows = () => [...container.querySelectorAll('.row.tasks-grid')]

describe('jobs', () => {
  it('says where an export went, since that is what somebody comes back for', async () => {
    tasks = [task({ result: { name: 'yinkote-20260101-120000.yinkote', bytes: 319_459_328 } })]
    await render()

    const text = rows()[0]?.textContent ?? ''
    expect(text).toContain('yinkote-20260101-120000.yinkote')
    // Bytes as a person reads them: the number this replaces was 319459328.
    expect(text).toContain('305 MB')
  })

  it('counts what an import added and what was already there', async () => {
    tasks = [task({ kind: 'import', result: { items: 12, skipped: 4000, files: 3 } })]
    await render()
    const text = rows()[0]?.textContent ?? ''
    expect(text).toContain('12')
    expect(text).toContain('4000')
  })

  it('shows a failure with its reason rather than a bare word', async () => {
    tasks = [task({ kind: 'backup', phase: 'failed', error: 'the disk is full' })]
    await render()
    expect(rows()[0]?.textContent).toContain('the disk is full')
    expect(rows()[0]?.getAttribute('data-phase')).toBe('failed')
  })

  it('shows progress for a job that can count, and no percentage for one that cannot', async () => {
    tasks = [
      task({ id: 'a', kind: 'import', phase: 'running', message: 'Restoring', done: 30, total: 120 }),
      task({ id: 'b', kind: 'reindex', phase: 'running', message: 'Rebuilding', done: 0, total: 0 }),
    ]
    await render()

    expect(rows()[0]?.textContent).toContain('25%')
    // A rebuild cannot count its work. A bar that invents a percentage is worse
    // than none, because it is believed.
    expect(rows()[1]?.textContent).toContain('Rebuilding')
    expect(rows()[1]?.textContent).not.toContain('%')
  })

  it('offers to stop only what is still running', async () => {
    tasks = [
      task({ id: 'going', kind: 'zotero', phase: 'running', message: 'Importing' }),
      task({ id: 'over', kind: 'backup', phase: 'done' }),
    ]
    await render()

    expect(rows()[0]?.querySelector('button')).not.toBeNull()
    expect(rows()[1]?.querySelector('button')).toBeNull()

    const stop = rows()[0]?.querySelector('button') as HTMLButtonElement
    await act(async () => stop.click())
    expect(cancelled).toEqual(['going'])
  })
})
