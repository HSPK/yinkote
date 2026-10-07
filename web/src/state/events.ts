import { connectEvents } from '../api/client'

interface Handlers {
  library: () => number
  connected: (connected: boolean) => void
  refresh: () => Promise<void>
  sidebar: (scope: 'all' | 'library' | 'plugins') => Promise<void>
  run: (conversation: string, state: unknown) => void
  pluginsChanged: () => void
}

/** Coalesce bursts without postponing refresh forever under sustained writes. */
export function watchEvents(handlers: Handlers): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined
  let items = false
  let scope: 'all' | 'library' | 'plugins' | undefined
  const schedule = (next: 'all' | 'library' | 'plugins') => {
    items ||= next !== 'plugins'
    scope = scope && scope !== next ? 'all' : next
    if (timer !== undefined) return
    timer = setTimeout(() => {
      timer = undefined
      const reloadItems = items
      const reloadSidebar = scope
      items = false
      scope = undefined
      if (reloadItems) void handlers.refresh()
      if (reloadSidebar) void handlers.sidebar(reloadSidebar)
    }, 150)
  }
  const close = connectEvents((event) => {
    const type = String(event.type)
    if (type === 'connected') {
      handlers.connected(true)
      // Initial HTTP reads may finish before the WebSocket subscribes too.
      schedule('all')
    } else if (type === 'disconnected') {
      handlers.connected(false)
    } else if (type === 'lagged') {
      schedule('all')
    } else if (event.libraryId !== undefined && event.libraryId !== handlers.library()) {
      return
    } else if (type.startsWith('items') || type === 'collectionsChanged' || type === 'tagsChanged') {
      schedule('library')
    } else if (type === 'agentProgress') {
      handlers.run(String(event.conversation ?? ''), event.state)
    } else if (type === 'pluginsChanged') {
      handlers.pluginsChanged()
      schedule('plugins')
    }
  })
  return () => {
    close()
    if (timer !== undefined) clearTimeout(timer)
  }
}
