import { useStore } from '../state/store'

export function StatusBar() {
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
      <span className="dot" data-on={connected} title={connected ? '实时连接正常' : '实时连接断开'}>
        ●
      </span>
      <span>{connected ? 'LIVE' : 'OFFLINE'}</span>
      <span>{total} 条</span>
      {selected.length > 0 && <span>已选 {selected.length}</span>}
      {query && <span>{mode.toUpperCase()} · {tookMs}ms</span>}
      {!query && <span>{tookMs}ms</span>}

      <span className="spacer" />

      {error && <span className="err" title={error}>⚠ {error}</span>}
      {stats && <span>向量 {stats.search.embedded}/{stats.search.documents}</span>}

      {(['detail', 'plugins', 'stats'] as const).map((p) => (
        <button key={p} className="toolbtn" data-active={panel === p} onClick={() => setPanel(p)}>
          {p === 'detail' ? '详情' : p === 'plugins' ? '插件' : '状态'}
        </button>
      ))}
    </footer>
  )
}
