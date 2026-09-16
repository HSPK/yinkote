import { useT } from '../i18n'
import { useStore } from '../state/store'
import { Icon } from '../ui'
import { tabId } from '../lib/tabs'
import { QuickAdd } from './QuickAdd'
import { SearchBar } from './SearchBar'
import { TABS } from '../workspace/registry'

/**
 * A thin strip, not a navigation bar.
 *
 * Search sits in the middle because it is the thing reached for most; the
 * secondary surfaces are icons on the right, out of the way until wanted.
 */
export function TopBar() {
  const t = useT()
  const openSettings = useStore((s) => s.openSettings)
  const openTab = useStore((s) => s.openTab)
  const sidebarOpen = useStore((s) => s.sidebarOpen)
  const toggleSidebar = useStore((s) => s.toggleSidebar)
  const activeTab = useStore((s) => s.tabs.find((tab) => tab.id === s.activeTab))
  const detailOpen = useStore((s) => s.detailOpen)
  const readerDetailsOpen = useStore((s) => s.readerLayout.detailsOpen)
  const toggleDetail = useStore((s) => s.toggleDetail)
  const canInspect = activeTab ? Boolean(TABS[activeTab.kind].withDetail) : false
  const inspecting = canInspect && (activeTab?.kind === 'reader' ? readerDetailsOpen : detailOpen)

  return (
    <header className="toolbar">
      <div className="toolbar-left">
        <button className="icon-btn" aria-expanded={sidebarOpen} aria-controls="library-sidebar"
          title={t(sidebarOpen ? 'sidebar.hide' : 'sidebar.show')}
          aria-label={t(sidebarOpen ? 'sidebar.hide' : 'sidebar.show')}
          onClick={toggleSidebar}>
          <Icon.Panel size={14} />
        </button>
        <span className="brand">YINKOTE</span>
      </div>

      <div className="toolbar-centre">
        <SearchBar />
      </div>

      <div className="toolbar-right">
        <QuickAdd />
        <button className="icon-btn" disabled={!canInspect} aria-pressed={inspecting}
          aria-controls="workspace-details"
          title={t(!canInspect ? 'detail.unavailable' : inspecting ? 'detail.hide' : 'detail.show')}
          aria-label={t(inspecting ? 'detail.hide' : 'detail.show')}
          onClick={() => toggleDetail()}>
          <Icon.Panel size={14} />
        </button>
        <button
          className="icon-btn"
          title={t('nav.plugins')}
          onClick={() => openTab({ id: tabId('plugins'), kind: 'plugins', title: '' })}
        >
          <Icon.Plugin />
        </button>
        <button
          className="icon-btn"
          title={t('nav.status')}
          onClick={() => openTab({ id: tabId('status'), kind: 'status', title: '' })}
        >
          <Icon.Gauge />
        </button>
        <button className="icon-btn" title={t('nav.settings')} onClick={() => openSettings()}>
          <Icon.Settings />
        </button>
      </div>
    </header>
  )
}
