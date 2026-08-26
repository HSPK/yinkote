import { useEffect } from 'react'

import { CommandPalette } from './components/CommandPalette'
import { SmartEditorHost } from './components/SmartEditorHost'
import { DetailPanel } from './components/DetailPanel'
import { ItemTable } from './components/ItemTable'
import { Sidebar } from './components/Sidebar'
import { StatusBar } from './components/StatusBar'
import { TopBar } from './components/TopBar'
import { useT } from './i18n'
import { ChatView } from './pages/ChatView'
import { PluginsPage } from './pages/PluginsPage'
import { SettingsPage } from './pages/SettingsPage'
import { StatusPage } from './pages/StatusPage'
import { useStore } from './state/store'
import { Modal, OverlayHost, Splitter } from './ui'

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
      if (e.key === 'Escape') {
        if (store.paletteOpen) return store.togglePalette(false)
        if (store.modal) return store.setModal(null)
      }
      if (isEditing(e.target) || store.modal) return

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
        case 'i':
          e.preventDefault()
          store.toggleDetail()
          break
        default:
          break
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [store])
}

const MODALS = {
  plugins: { title: 'nav.plugins', width: 'wide', Body: PluginsPage },
  status: { title: 'nav.status', width: 'wide', Body: StatusPage },
  settings: { title: 'nav.settings', width: 'narrow', Body: SettingsPage },
} as const

export function App() {
  const t = useT()
  const ready = useStore((s) => s.ready)
  const error = useStore((s) => s.error)
  const view = useStore((s) => s.view)
  const modal = useStore((s) => s.modal)
  const layout = useStore((s) => s.layout)
  const detailOpen = useStore((s) => s.detailOpen)
  const setModal = useStore((s) => s.setModal)
  const setLayout = useStore((s) => s.setLayout)
  const bootstrap = useStore((s) => s.bootstrap)

  useEffect(() => {
    void bootstrap()
  }, [bootstrap])

  useGlobalKeys()

  if (!ready) {
    return (
      <div className="empty" style={{ paddingTop: '20vh' }}>
        {t('app.connecting')}
      </div>
    )
  }

  const open = modal ? MODALS[modal] : null

  return (
    <div className="app">
      <TopBar />
      {error && <div className="banner">{error}</div>}

      <div className="workspace">
        <div className="pane sidebar" style={{ width: layout.sidebar }}>
          <Sidebar />
        </div>
        <Splitter
          size={layout.sidebar}
          min={180}
          max={420}
          grows="left"
          onResize={(sidebar) => setLayout({ sidebar })}
          onCommit={(sidebar) => setLayout({ sidebar }, true)}
        />

        {view === 'chat' ? <ChatView /> : <ItemTable />}

        {view !== 'chat' && detailOpen && (
          <>
            <Splitter
              size={layout.detail}
              min={260}
              max={640}
              grows="right"
              onResize={(detail) => setLayout({ detail })}
              onCommit={(detail) => setLayout({ detail }, true)}
            />
            <div className="pane detail-pane" style={{ width: layout.detail }}>
              <DetailPanel />
            </div>
          </>
        )}
      </div>

      <StatusBar />
      <CommandPalette />
      <SmartEditorHost />
      {open && (
        <Modal title={t(open.title)} width={open.width} onClose={() => setModal(null)}>
          <open.Body />
        </Modal>
      )}
      <OverlayHost />
    </div>
  )
}
