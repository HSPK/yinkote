import { afterEach, describe, expect, it, vi } from 'vitest'

import { api, ApiError, buildQuery } from './client'

describe('buildQuery', () => {
  it('is empty for an empty query', () => {
    expect(buildQuery({})).toBe('')
  })

  it('omits undefined and empty values rather than sending blanks', () => {
    expect(buildQuery({ q: '', collection: undefined, limit: undefined })).toBe('')
  })

  it('encodes scalars', () => {
    expect(buildQuery({ q: 'attention', limit: 50 })).toBe('?q=attention&limit=50')
  })

  it('joins list filters with commas, matching the server contract', () => {
    expect(buildQuery({ tag: ['a', 'b'], itemType: ['book'] })).toBe('?tag=a%2Cb&itemType=book')
  })

  it('percent-encodes user input', () => {
    expect(buildQuery({ q: 'tag:综述 & more' })).toContain('q=tag%3A%E7%BB%BC%E8%BF%B0')
  })

  it('keeps zero, which is a meaningful offset', () => {
    expect(buildQuery({ offset: 0 })).toBe('?offset=0')
  })
})

describe('request handling', () => {
  afterEach(() => vi.unstubAllGlobals())

  const stub = (status: number, body: unknown) =>
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(typeof body === 'string' ? body : JSON.stringify(body), {
          status,
          headers: { 'content-type': 'application/json' },
        }),
      ),
    )

  it('returns parsed JSON on success', async () => {
    stub(200, { ok: true, version: '0.1.0', defaultLibrary: 1 })
    await expect(api.ping()).resolves.toMatchObject({ ok: true })
  })

  it('raises ApiError carrying the server code', async () => {
    stub(404, { code: 'not_found', title: 'item ZZZZ' })
    await expect(api.items.get(1, 'ZZZZ')).rejects.toMatchObject({
      name: 'ApiError',
      status: 404,
      code: 'not_found',
      message: 'item ZZZZ',
    })
  })

  it('still raises when the error body is not JSON', async () => {
    stub(500, 'boom')
    await expect(api.stats()).rejects.toBeInstanceOf(ApiError)
  })

  /** Typed so the recorded call arguments keep their shape. */
  const spyFetch = (body = '{}') =>
    vi.fn(async (_url: string, _init?: RequestInit) => new Response(body, { status: 200 }))

  it('sends the optimistic-locking header when a version is supplied', async () => {
    const fetchMock = spyFetch()
    vi.stubGlobal('fetch', fetchMock)

    await api.items.update(1, 'ABCD1234', { fields: { volume: '1' } }, 42)

    const init = fetchMock.mock.calls[0]![1]!
    expect((init.headers as Record<string, string>)['If-Unmodified-Since-Version']).toBe('42')
    expect(init.method).toBe('PATCH')
  })

  it('omits the header when no version is supplied', async () => {
    const fetchMock = spyFetch()
    vi.stubGlobal('fetch', fetchMock)

    await api.items.update(1, 'ABCD1234', { fields: {} })

    const init = fetchMock.mock.calls[0]![1]!
    expect((init.headers as Record<string, string>)['If-Unmodified-Since-Version']).toBeUndefined()
  })

  it('targets the versioned API prefix', async () => {
    const fetchMock = spyFetch('[]')
    vi.stubGlobal('fetch', fetchMock)

    await api.collections.list(3)

    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/v1/libraries/3/collections')
  })
})

describe('citations', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('asks the server to render, sending keys rather than items', async () => {
    const fetchMock = vi.fn(
      async (_url: RequestInfo | URL, _init?: RequestInit) =>
        new Response(JSON.stringify({ style: 'apa', citations: [], bibliography: ['x'] }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
    )
    vi.stubGlobal('fetch', fetchMock)

    await api.citations.render(1, ['AAAA1111', 'BBBB2222'], 'gb7714', 'html')

    const [url, init] = fetchMock.mock.calls[0]!
    expect(String(url)).toContain('/libraries/1/citations')
    // The server already has the items; sending them back would let the client
    // decide what a reference says, which is exactly what it must not do.
    expect(JSON.parse(String(init?.body))).toEqual({
      keys: ['AAAA1111', 'BBBB2222'],
      style: 'gb7714',
      format: 'html',
    })
  })
})

it('has no method nothing calls', async () => {
  // A client exists to be called, so an uncalled method is either dead or a
  // feature that was never wired up. Both were true here: `search` and
  // `libraries` had no caller, and `search`'s return type had drifted out of
  // step with the endpoint — which is what happens to code nothing exercises.
  //
  // This also fixes a bad audit. A previous round checked "does the workbench
  // reference this route" by searching all of `src`, which includes the client
  // itself — so every endpoint looked reached, by its own wrapper. The
  // question has to be asked one layer up.
  const { join } = await import('node:path')
  const { sourceFiles } = await import('../test/sources')

  const clientPath = join('src', 'api', 'client.ts')
  const files = sourceFiles()
  const client = files.find(({ path }) => path === clientPath)?.text ?? ''
  const names = [
    ...new Set(
      [...client.matchAll(/^\s{2,}([a-zA-Z][a-zA-Z0-9_]*):\s*(?:\(|async\s*\()/gm)].map(
        (m) => m[1] ?? '',
      ),
    ),
  ]
  const callers = files
    .filter(({ path }) => path !== clientPath)
    .map(({ text }) => text)
    .join('\n')

  const uncalled = names.filter((n) => !new RegExp(`\\.${n}\\s*\\(`).test(callers))
  expect(uncalled, `client methods with no caller: ${uncalled.join(', ')}`).toEqual([])
})
