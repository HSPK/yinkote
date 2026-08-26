import { useEffect } from 'react'

import { CommandPalette } from './components/CommandPalette'
import { NavRail } from './components/NavRail'
import { StatusBar } from './components/StatusBar'
import { TopBar } from './components/TopBar'
import { onNavigate, pageFromHash } from './lib/router'
import { ChatPage } from './pages/ChatPage'
import { LibraryPage } from './pages/LibraryPage'
import { PluginsPage } from './pages/PluginsPage'
import { SettingsPage } from './pages/SettingsPage'
import { StatusPage } from './pages/StatusPage'
import { useStore } from './state/store'
import { OverlayHost } from './ui'

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
        case 'a':
          e.preventDefault()
          document.getElementById('quick-add-input')?.focus()
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

const PAGES = {
  library: LibraryPage,
  chat: ChatPage,
  plugins: PluginsPage,
  status: StatusPage,
  settings: SettingsPage,
}

export function App() {
  const ready = useStore((s) => s.ready)
  const error = useStore((s) => s.error)
  const page = useStore((s) => s.page)
  const setPage = useStore((s) => s.setPage)
  const bootstrap = useStore((s) => s.bootstrap)

  useEffect(() => {
    void bootstrap()
  }, [bootstrap])

  // The hash is the source of truth, so Back and deep links both work.
  useEffect(() => {
    setPage(pageFromHash())
    return onNavigate(setPage)
  }, [setPage])

  useGlobalKeys()

  if (!ready) {
    return <div className="empty" style={{ paddingTop: '20vh' }}>正在连接 Yinkote 服务…</div>
  }

  const Current = PAGES[page]

  return (
    <div className="app">
      <TopBar />
      {error && <div className="banner">{error}</div>}
      <div className="shell">
        <NavRail />
        <main className="page-host">
          <Current />
        </main>
      </div>
      <StatusBar />
      <CommandPalette />
      <OverlayHost />
    </div>
  )
}
