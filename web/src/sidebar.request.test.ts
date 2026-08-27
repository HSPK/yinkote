/**
 * What the sidebar asks the server for.
 *
 * The shape of this request is load-bearing in a way that is easy to miss:
 * the server warms the facet cache at startup using its *own* default limit,
 * so a client that sends a different one warms a slot nobody asks for and
 * pays the full count on the first load — 264ms on a hundred-thousand-item
 * library, against 3ms.
 *
 * That happened. Twice, in the same feature: once because the filter was
 * hand-built from the type instead of the route's conversion, and once
 * because the client hard-coded 80 while the warm-up computed 60. Both were
 * invisible — the cache misses quietly and everything still works.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'

const facetCalls: unknown[] = []

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: (_t, _this, args: unknown[]) => {
        if (path === 'api.tags.facets') {
          facetCalls.push(args[1])
          return Promise.resolve([])
        }
        if (path === 'api.agent') return Promise.resolve({ configured: false })
        return Promise.resolve([])
      },
    })
  return { api: build('api'), connectEvents: () => () => {} }
})

beforeEach(() => {
  facetCalls.length = 0
})

describe('the sidebar request', () => {
  it('lets the server choose how many facets to return', async () => {
    const { useStore } = await import('./state/store')
    await useStore.getState().reloadSidebar()

    // A `limit` here is a second opinion about a number the server already
    // has — and the startup warm-up computes the server's one.
    expect(facetCalls).toHaveLength(1)
    expect(facetCalls[0]).not.toHaveProperty('limit')
  })

  it('still narrows to the view being shown', async () => {
    const { useStore } = await import('./state/store')
    useStore.setState({ view: 'trash' })
    await useStore.getState().reloadSidebar()

    // Dropping the limit must not drop the filter: the trash view's tags are
    // not the library's tags.
    expect(facetCalls[0]).toMatchObject({ trash: 'only' })
  })
})
