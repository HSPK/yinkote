import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const feed = vi.hoisted(() => ({
  receive: (_event: Record<string, unknown>) => {},
  close: vi.fn(),
}))
vi.mock('../api/client', () => ({
  connectEvents: (receive: typeof feed.receive) => {
    feed.receive = receive
    return feed.close
  },
}))
import { watchEvents } from './events'

beforeEach(() => {
  vi.useFakeTimers()
  vi.clearAllMocks()
})
afterEach(() => vi.useRealTimers())

function start() {
  const handlers = {
    library: () => 1,
    connected: vi.fn(),
    refresh: vi.fn(async () => {}),
    sidebar: vi.fn(async (_scope: 'all' | 'library' | 'plugins') => {}),
    run: vi.fn(),
    pluginsChanged: vi.fn(),
  }
  return { ...handlers, close: watchEvents(handlers) }
}

describe('live library synchronization', () => {
  it('coalesces a burst and avoids reloading unrelated plugin data', async () => {
    const handlers = start()
    for (let i = 0; i < 100; i++) feed.receive({ type: 'itemsChanged', libraryId: 1 })
    await vi.advanceTimersByTimeAsync(150)
    expect(handlers.refresh).toHaveBeenCalledTimes(1)
    expect(handlers.sidebar).toHaveBeenCalledExactlyOnceWith('library')
    handlers.close()
  })

  it('resynchronizes on connection, reconnection and lost events', async () => {
    const handlers = start()
    feed.receive({ type: 'connected' })
    await vi.advanceTimersByTimeAsync(150)
    expect(handlers.refresh).toHaveBeenCalledOnce()
    expect(handlers.sidebar).toHaveBeenCalledExactlyOnceWith('all')
    handlers.refresh.mockClear()
    handlers.sidebar.mockClear()
    feed.receive({ type: 'disconnected' })
    feed.receive({ type: 'connected' })
    await vi.advanceTimersByTimeAsync(150)
    expect(handlers.sidebar).toHaveBeenCalledExactlyOnceWith('all')
    feed.receive({ type: 'lagged', missed: 10 })
    await vi.advanceTimersByTimeAsync(150)
    expect(handlers.refresh).toHaveBeenCalledTimes(2)
    handlers.close()
  })

  it('refreshes only contributions after a plugin change', async () => {
    const handlers = start()
    feed.receive({ type: 'pluginsChanged' })
    await vi.advanceTimersByTimeAsync(150)
    expect(handlers.pluginsChanged).toHaveBeenCalledOnce()
    expect(handlers.sidebar).toHaveBeenCalledExactlyOnceWith('plugins')
    expect(handlers.refresh).not.toHaveBeenCalled()
    handlers.close()
  })

  it('ignores other libraries and cancels pending refresh on teardown', async () => {
    const handlers = start()
    feed.receive({ type: 'itemsChanged', libraryId: 2 })
    await vi.advanceTimersByTimeAsync(150)
    expect(handlers.refresh).not.toHaveBeenCalled()
    feed.receive({ type: 'itemsChanged', libraryId: 1 })
    handlers.close()
    await vi.advanceTimersByTimeAsync(150)
    expect(handlers.refresh).not.toHaveBeenCalled()
    expect(feed.close).toHaveBeenCalledOnce()
  })
})
