import { PAGE_LABELS, PAGES, navigate, type Page } from '../lib/router'
import { useStore } from '../state/store'

/** Primary navigation. A narrow rail keeps the workbench's horizontal space
 *  for content while making every page one click away. */
export function NavRail() {
  const page = useStore((s) => s.page)
  const stats = useStore((s) => s.stats)
  const connected = useStore((s) => s.connected)

  const badge = (id: Page): string | null => {
    if (!stats) return null
    if (id === 'library') return String(stats.items)
    if (id === 'plugins') return stats.plugins ? String(stats.plugins) : null
    return null
  }

  return (
    <nav className="rail">
      {PAGES.map((id) => {
        const { label, glyph } = PAGE_LABELS[id]
        const count = badge(id)
        return (
          <button
            key={id}
            className="rail-item"
            data-active={page === id}
            title={label}
            onClick={() => navigate(id)}
          >
            <span className="rail-glyph">{glyph}</span>
            <span className="rail-label">{label}</span>
            {count && <span className="rail-count">{count}</span>}
          </button>
        )
      })}
      <div className="rail-spacer" />
      <div className="rail-status" data-on={connected} title={connected ? '实时连接正常' : '实时连接断开'}>
        ●
      </div>
    </nav>
  )
}
