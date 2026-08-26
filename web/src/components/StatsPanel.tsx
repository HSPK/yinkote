import { compact } from '../lib/format'
import { useStore } from '../state/store'

export function StatsPanel() {
  const stats = useStore((s) => s.stats)
  const reindex = useStore((s) => s.reindex)
  const tookMs = useStore((s) => s.tookMs)

  return (
    <aside className="pane">
      <div className="pane-header">
        运行状态
        <span className="spacer" />
        <button className="toolbtn" onClick={() => void reindex()} title="重建全文与向量索引">
          重建索引
        </button>
      </div>

      {!stats ? (
        <div className="empty">载入中…</div>
      ) : (
        <dl className="kv">
          <dt>条目</dt>
          <dd>{compact(stats.items)}</dd>
          <dt>回收站</dt>
          <dd>{compact(stats.trashed)}</dd>
          <dt>收藏夹</dt>
          <dd>{stats.collections}</dd>
          <dt>标签</dt>
          <dd>{stats.tags}</dd>
          <dt>库版本</dt>
          <dd>{stats.version}</dd>

          <dt style={{ paddingTop: 12 }}>索引文档</dt>
          <dd style={{ paddingTop: 12 }}>{compact(stats.search.documents)}</dd>
          <dt>已生成向量</dt>
          <dd>{compact(stats.search.embedded)}</dd>
          <dt>向量维度</dt>
          <dd>{stats.search.dimensions}</dd>
          <dt>嵌入提供方</dt>
          <dd>{stats.search.provider}</dd>

          <dt style={{ paddingTop: 12 }}>插件</dt>
          <dd style={{ paddingTop: 12 }}>{stats.plugins}</dd>
          <dt>WS 客户端</dt>
          <dd>{stats.wsClients}</dd>
          <dt>运行时长</dt>
          <dd>{Math.floor(stats.uptimeSecs / 60)}m {stats.uptimeSecs % 60}s</dd>
          <dt>上次查询</dt>
          <dd>{tookMs}ms</dd>
        </dl>
      )}
    </aside>
  )
}
