import { useEffect } from 'react'

import { CommandPalette } from './components/CommandPalette'
import { DetailPanel } from './components/DetailPanel'
import { ItemTable } from './components/ItemTable'
import { PluginPanel } from './components/PluginPanel'
import { Sidebar } from './components/Sidebar'
import { StatsPanel } from './components/StatsPanel'
import { StatusBar } from './components/StatusBar'
import { TopBar } from './components/TopBar'
import { useStore } from './state/store'

/** True when a keystroke belongs to whatever the user is typing into. */
function isEditing(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null
  if (!el) return false
  return (
    el.tagName === 'INPUT' ||
    el.tagName === 'TEXTAREA' ||
    el.tagName === 'SELECT' ||
    el.isContentEditable
  )
}

function useGlobalKeys() {
  const store = useStore()
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey

      if (mod && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        store.togglePalette()
        return
      }
      if (e.key === 'Escape' && store.paletteOpen) {
        store.togglePalette(false)
        return
      }
      if (isEditing(e.target)) return

      switch (e.key) {
        case '/':
          e.preventDefault()
          document.getElementById('search-input')?.focus()
          break
        case 'j':
        case 'ArrowDown':
          e.preventDefault()
          store.moveCursor(1)
          break
        case 'k':
        case 'ArrowUp':
          e.preventDefault()
          store.moveCursor(-1)
          break
        case 'g':
          store.moveCursor(-1e9)
          break
        case 'G':
          store.moveCursor(1e9)
          break
        case 'Delete':
        case 'Backspace':
          if (store.selected.length) {
            e.preventDefault()
            void (store.view === 'trash' ? store.destroySelected() : store.trashSelected())
          }
          break
        case 'n':
          e.preventDefault()
          store.togglePalette(true)
          break
        default:
          break
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [store])
}

export function App() {
  const ready = useStore((s) => s.ready)
  const error = useStore((s) => s.error)
  const panel = useStore((s) => s.panel)
  const bootstrap = useStore((s) => s.bootstrap)

  useEffect(() => {
    void bootstrap()
  }, [bootstrap])

  useGlobalKeys()

  if (!ready) {
    return <div className="empty" style={{ paddingTop: '20vh' }}>正在连接 Yinkote 服务…</div>
  }

  return (
    <div className="app">
      <TopBar />
      {error && <div className="banner">{error}</div>}
      <div className="workspace">
        <Sidebar />
        <ItemTable />
        {panel === 'detail' && <DetailPanel />}
        {panel === 'plugins' && <PluginPanel />}
        {panel === 'stats' && <StatsPanel />}
      </div>
      <StatusBar />
      <CommandPalette />
    </div>
  )
}
