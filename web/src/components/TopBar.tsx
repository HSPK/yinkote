import type { SearchMode } from '../api/types'
import { useT } from '../i18n'
import { useStore } from '../state/store'
import { Icon } from '../ui'
import { QuickAdd } from './QuickAdd'

const MODES: SearchMode[] = ['hybrid', 'keyword', 'fuzzy', 'semantic']

/**
 * A thin strip, not a navigation bar.
 *
 * Search sits in the middle because it is the thing reached for most; the
 * secondary surfaces are icons on the right, out of the way until wanted.
 */
export function TopBar() {
  const t = useT()
  const query = useStore((s) => s.query)
  const mode = useStore((s) => s.mode)
  const setQuery = useStore((s) => s.setQuery)
  const setMode = useStore((s) => s.setMode)
  const setModal = useStore((s) => s.setModal)

  return (
    <header className="toolbar">
      <div className="toolbar-left">
        <span className="brand">YINKOTE</span>
      </div>

      <div className="toolbar-centre">
        <div className="search">
          <Icon.Search size={12} className="search-icon" />
          <input
            id="search-input"
            value={query}
            spellCheck={false}
            autoComplete="off"
            placeholder={t('search.placeholder')}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                setQuery('')
                e.currentTarget.blur()
              }
            }}
          />
          <div className="modes">
            {MODES.map((m) => (
              <button
                key={m}
                title={t(`search.mode.${m}.hint`)}
                data-active={mode === m}
                onClick={() => setMode(m)}
              >
                {t(`search.mode.${m}`)}
              </button>
            ))}
          </div>
        </div>
      </div>

      <div className="toolbar-right">
        <QuickAdd />
        <button className="icon-btn" title={t('nav.plugins')} onClick={() => setModal('plugins')}>
          <Icon.Plugin />
        </button>
        <button className="icon-btn" title={t('nav.status')} onClick={() => setModal('status')}>
          <Icon.Gauge />
        </button>
        <button className="icon-btn" title={t('nav.settings')} onClick={() => setModal('settings')}>
          <Icon.Settings />
        </button>
      </div>
    </header>
  )
}
