import { useStore } from '../state/store'
import { useT } from '../i18n'

export function StatusBar() {
  const t = useT()
  const connected = useStore((s) => s.connected)
  const total = useStore((s) => s.total)
  const selected = useStore((s) => s.selected)
  const tookMs = useStore((s) => s.tookMs)
  const mode = useStore((s) => s.mode)
  const query = useStore((s) => s.query)
  const error = useStore((s) => s.error)
  const stats = useStore((s) => s.stats)
  const panel = useStore((s) => s.panel)
  const setPanel = useStore((s) => s.setPanel)

  return (
    <footer className="statusbar">
      <span className="dot" data-on={connected} title={connected ? t('status.connected') : t('status.disconnected')}>
        ●
      </span>
      <span>{connected ? t('status.live') : t('status.offline')}</span>
      <span>{t('status.items', { count: total })}</span>
      {selected.length > 0 && <span>{t('status.selected', { count: selected.length })}</span>}
      {query && <span>{mode.toUpperCase()} · {tookMs}ms</span>}
      {!query && <span>{tookMs}ms</span>}

      <span className="spacer" />

      {error && <span className="err" title={error}>⚠ {error}</span>}
      {stats && (
        <span>
          {t('status.vectors', {
            done: stats.search.embedded,
            total: stats.search.documents,
          })}
        </span>
      )}

      <button className="toolbtn" data-active={panel === 'detail'} onClick={() => setPanel('detail')}>
        {t('status.detailPanel')}
      </button>
    </footer>
  )
}
