import type { SearchMode } from '../api/types'
import { useT } from '../i18n'
import { PAGES, navigate } from '../lib/router'
import { useStore } from '../state/store'
import { QuickAdd } from './QuickAdd'

const MODES: SearchMode[] = ['hybrid', 'keyword', 'fuzzy', 'semantic']

/**
 * The only chrome: identity, navigation, search, quick add.
 *
 * Navigation lives here rather than in a side rail so the full window width
 * stays available for content — on a dense workbench that column is worth more
 * than the affordance it was buying.
 */
export function TopBar() {
  const t = useT()
  const page = useStore((s) => s.page)
  const query = useStore((s) => s.query)
  const mode = useStore((s) => s.mode)
  const stats = useStore((s) => s.stats)
  const setQuery = useStore((s) => s.setQuery)
  const setMode = useStore((s) => s.setMode)

  return (
    <header className="topbar">
      <div className="brand">
        YINKOTE<small>{t('app.brand.subtitle')}</small>
      </div>

      <nav className="tabs">
        {PAGES.map((id) => (
          <button key={id} data-active={page === id} onClick={() => navigate(id)}>
            {t(`nav.${id}`)}
            {id === 'library' && stats ? <span className="tab-count">{stats.items}</span> : null}
            {id === 'plugins' && stats?.plugins ? (
              <span className="tab-count">{stats.plugins}</span>
            ) : null}
          </button>
        ))}
      </nav>

      <div className="search">
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

      <QuickAdd />
    </header>
  )
}
