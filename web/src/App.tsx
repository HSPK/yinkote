import { useEffect } from 'react'

import { CommandPalette } from './components/CommandPalette'
import { CollectionEditorHost } from './components/CollectionEditorHost'
import { DetailPanel } from './components/DetailPanel'
import { Sidebar } from './components/Sidebar'
import { StatusBar } from './components/StatusBar'
import { TabBar } from './components/TabBar'
import { TopBar } from './components/TopBar'
import { TABS } from './workspace/registry'
import { useT } from './i18n'
import { SettingsPage } from './pages/SettingsPage'
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
      // One search box, so Ctrl+F goes to it rather than opening a second one
      // that searches something else.
      if (mod && e.key.toLowerCase() === 'f') {
        e.preventDefault()
        const box = document.getElementById('search-input') as HTMLInputElement | null
        box?.focus()
        box?.select()
        return
      }
      if (mod && e.key.toLowerCase() === 'a' && !isEditing(e.target)) {
        e.preventDefault()
        store.selectAll()
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

/** Only preferences stay modal. Everything you *work in* is a tab, because
 *  reading, annotating and asking happen alongside each other. */
const MODALS = {
  settings: { title: 'nav.settings', width: 'wide', scroll: false, Body: SettingsPage },
} as const

export function App() {
  const t = useT()
  const ready = useStore((s) => s.ready)
  const error = useStore((s) => s.error)
  const modal = useStore((s) => s.modal)
  const tabs = useStore((s) => s.tabs)
  const activeTab = useStore((s) => s.activeTab)
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
  const tab = tabs.find((t) => t.id === activeTab) ?? tabs[0]
  const current = tab ? { tab, def: TABS[tab.kind] } : null
  const showDetail = current?.def.withDetail ?? false

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

        <div className="workspace-main">
          <TabBar />
          {current && <current.def.Body target={current.tab.target} />}
        </div>

        {showDetail && detailOpen && (
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
      <CollectionEditorHost />
      {open && (
        <Modal
          title={t(open.title)}
          width={open.width}
          scroll={open.scroll}
          onClose={() => setModal(null)}
        >
          <open.Body />
        </Modal>
      )}
      <OverlayHost />
    </div>
  )
}
