import { useT } from '../i18n'
import { useStore } from '../state/store'
import { Icon } from '../ui'
import { tabId } from '../lib/tabs'
import { QuickAdd } from './QuickAdd'
import { SearchBar } from './SearchBar'

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

  return (
    <header className="toolbar">
      <div className="toolbar-left">
        <span className="brand">YINKOTE</span>
      </div>

      <div className="toolbar-centre">
        <SearchBar />
      </div>

      <div className="toolbar-right">
        <QuickAdd />
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
