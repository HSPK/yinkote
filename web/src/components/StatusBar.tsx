import { ActivityIndicator } from './ActivityIndicator'
import { useStore } from '../state/store'
import { useT } from '../i18n'
import { TABS } from '../workspace/registry'

export function StatusBar() {
  const t = useT()
  const connected = useStore((s) => s.connected)
  const selected = useStore((s) => s.selected)
  const tookMs = useStore((s) => s.tookMs)
  const mode = useStore((s) => s.mode)
  const query = useStore((s) => s.query)
  const error = useStore((s) => s.error)
  const stats = useStore((s) => s.stats)
  const tabs = useStore((s) => s.tabs)
  const activeTab = useStore((s) => s.activeTab)

  const active = tabs.find((t) => t.id === activeTab)
  const Footer = active ? TABS[active.kind].Footer : undefined

  return (
    <footer className="statusbar">
      <span className="dot" data-on={connected} title={connected ? t('status.connected') : t('status.disconnected')}>
        ●
      </span>
      <span>{connected ? t('status.live') : t('status.offline')}</span>
      <ActivityIndicator />
      {selected.length > 0 && <span>{t('status.selected', { count: selected.length })}</span>}
      {query && (
        <span>
          {mode.toUpperCase()} · {tookMs}ms
        </span>
      )}
      {error && (
        <span className="err" title={error}>
          ⚠ {error}
        </span>
      )}
      {stats?.search && (
        <span className="dim">
          {t('status.vectors', {
            done: stats.search.embedded,
            total: stats.search.documents,
          })}
        </span>
      )}

      <span className="statusbar-sep" />
      {Footer ? <Footer /> : <span className="spacer" />}
    </footer>
  )
}
