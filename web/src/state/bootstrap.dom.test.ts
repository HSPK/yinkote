/**
 * What starting up asks the server for.
 *
 * Bootstrap fetched collections, then called `reloadSidebar`, which fetches
 * them again a millisecond later and writes the same state. The duplicate cost
 * little on its own; what made it worth removing is that both run again on
 * every change event, so the library paid for it all session long.
 *
 * The test counts calls rather than reading the source, because the question
 * is how many requests reach the server, not how the code is spelled.
 *
 * A `.dom.` test because bootstrap restores preferences, which touches the
 * document. Without one it fails at "document is not defined" inside the try,
 * and the call counts come out convincingly small for the wrong reason.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'

const calls: string[] = []

vi.mock('../api/client', () => {
  const answers: Record<string, unknown> = {
    'api.ping': { defaultLibrary: 1, ok: true },
    'api.schema': { itemTypes: [], fields: {} },
    'api.settings.get': {},
    'api.citations.styles': [],
    'api.collections.list': [],
    'api.smart.list': [],
    'api.conversations.list': [],
    'api.tags.facets': [],
    'api.stats': { items: 0 },
    'api.plugins.list': [],
    'api.badges.descriptors': [],
    'api.agent': { configured: false },
    'api.items.list': { items: [], total: 0, offset: 0, limit: 200 },
  }
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: () => {
        calls.push(path)
        return Promise.resolve(answers[path] ?? {})
      },
    })
  class ApiError extends Error {}
  return { api: build('api'), connectEvents: () => () => {}, ApiError }
})

const { useStore } = await import('./store')

beforeEach(() => {
  calls.length = 0
})

describe('starting up', () => {
  it('asks for each thing once', async () => {
    await useStore.getState().bootstrap()

    const counted = calls.reduce<Record<string, number>>((acc, c) => {
      acc[c] = (acc[c] ?? 0) + 1
      return acc
    }, {})
    const twice = Object.entries(counted).filter(([, n]) => n > 1)
    expect(twice, 'the same request is made more than once at start').toEqual([])
  })

  it('still ends up with everything it needs to paint', async () => {
    // Removing a duplicate must not remove the only copy: collections come
    // from reloadSidebar now, and the sidebar is allowed to fail quietly, so
    // this is the check that they arrive at all.
    await useStore.getState().bootstrap()

    expect(calls).toContain('api.collections.list')
    expect(calls).toContain('api.schema')
    expect(calls).toContain('api.items.list')
    expect(useStore.getState().ready).toBe(true)
  })
})
