import { useStore } from '../state/store'

export function PluginPanel() {
  const plugins = useStore((s) => s.plugins)
  const setEnabled = useStore((s) => s.setPluginEnabled)
  const reload = useStore((s) => s.reloadPlugins)

  return (
    <aside className="pane">
      <div className="pane-header">
        插件 · {plugins.length}
        <span className="spacer" />
        <button className="toolbtn" onClick={() => void reload()}>
          重新扫描
        </button>
      </div>

      {plugins.length === 0 && (
        <div className="empty">
          尚未安装插件
          <br />
          <br />
          把插件目录放到
          <br />
          <code>&lt;data-dir&gt;/plugins/&lt;name&gt;/</code>
          <br />
          并包含 <code>plugin.json</code>
        </div>
      )}

      {plugins.map((p) => (
        <div className="plugin" key={p.id}>
          <div className="plugin-head">
            <span className="name">{p.name}</span>
            <span className="version">{p.version}</span>
            <span className="state" data-s={p.state}>
              {p.state}
            </span>
            <button
              className="switch"
              onClick={() => void setEnabled(p.id, p.state === 'disabled')}
            >
              {p.state === 'disabled' ? '启用' : '停用'}
            </button>
          </div>

          {p.description && <p className="desc">{p.description}</p>}
          {p.error && <p className="desc" style={{ color: 'var(--err)' }}>{p.error}</p>}

          <div className="meta">
            <span>id={p.id}</span>
            {p.permissions.length > 0 && <span>权限: {p.permissions.join(' ')}</span>}
            {p.hooks.length > 0 && <span>钩子: {p.hooks.join(' ')}</span>}
            <span>
              调用 {p.calls} · 失败 {p.failures} · {p.avgLatencyMs.toFixed(0)}ms
            </span>
          </div>

          {p.contributions.metadataSources.length > 0 && (
            <div className="meta">
              数据源: {p.contributions.metadataSources.map((s) => s.label).join(', ')}
            </div>
          )}
        </div>
      ))}
    </aside>
  )
}
