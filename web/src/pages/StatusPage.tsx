import { compact } from '../lib/format'
import { useStore } from '../state/store'
import { Button, Section, withToast } from '../ui'

function Metric({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="metric">
      <span className="metric-value">{value}</span>
      <span className="metric-label">{label}</span>
      {hint && <span className="metric-hint">{hint}</span>}
    </div>
  )
}

function duration(seconds: number): string {
  const d = Math.floor(seconds / 86400)
  const h = Math.floor((seconds % 86400) / 3600)
  const m = Math.floor((seconds % 3600) / 60)
  if (d) return `${d}天 ${h}小时`
  if (h) return `${h}小时 ${m}分`
  return `${m}分 ${seconds % 60}秒`
}

/** Everything about the running service in one place: corpus size, index
 *  health, connectivity and the maintenance actions that fix them. */
export function StatusPage() {
  const stats = useStore((s) => s.stats)
  const server = useStore((s) => s.server)
  const tookMs = useStore((s) => s.tookMs)
  const connected = useStore((s) => s.connected)
  const reindex = useStore((s) => s.reindex)
  const optimize = useStore((s) => s.optimize)
  const reloadSidebar = useStore((s) => s.reloadSidebar)

  const embedded = stats?.search.embedded ?? 0
  const documents = stats?.search.documents ?? 0
  const coverage = documents === 0 ? 100 : Math.round((embedded / documents) * 100)

  return (
    <div className="page">
      <Section
        title="文库"
        action={<Button tone="ghost" onClick={() => void reloadSidebar()}>刷新</Button>}
      >
        <div className="metrics">
          <Metric label="条目" value={compact(stats?.items ?? 0)} />
          <Metric label="回收站" value={compact(stats?.trashed ?? 0)} />
          <Metric label="收藏夹" value={String(stats?.collections ?? 0)} />
          <Metric label="标签" value={String(stats?.tags ?? 0)} />
          <Metric label="库版本" value={String(stats?.version ?? 0)} hint="每次写入自增" />
        </div>
      </Section>

      <Section
        title="搜索索引"
        action={
          <Button
            onClick={() =>
              withToast(reindex, { success: '索引已重建', failure: '重建索引失败' })
            }
          >
            重建索引
          </Button>
        }
      >
        <div className="metrics">
          <Metric label="已索引文档" value={compact(documents)} />
          <Metric
            label="向量覆盖率"
            value={`${coverage}%`}
            hint={`${compact(embedded)} / ${compact(documents)}`}
          />
          <Metric label="向量维度" value={String(stats?.search.dimensions ?? 0)} />
          <Metric label="嵌入提供方" value={stats?.search.provider ?? '—'} />
          <Metric label="上次查询" value={`${tookMs}ms`} />
        </div>
        {coverage < 100 && (
          <p className="note">
            后台正在补齐向量，语义搜索的召回会随之提升。写入期间它会主动让出数据库写锁，
            所以不会影响你的编辑速度。
          </p>
        )}
      </Section>

      <Section
        title="服务"
        action={
          <Button
            onClick={() =>
              withToast(optimize, { success: '数据库已优化', failure: '优化失败' })
            }
          >
            优化数据库
          </Button>
        }
      >
        <dl className="kv">
          <dt>版本</dt>
          <dd>{server?.version ?? '—'}</dd>
          <dt>API 版本</dt>
          <dd>v{server?.apiVersion ?? '—'}</dd>
          <dt>插件协议</dt>
          <dd>v{server?.pluginApiVersion ?? '—'}</dd>
          <dt>监听地址</dt>
          <dd>{server?.bind ?? '—'}</dd>
          <dt>运行时长</dt>
          <dd>{duration(stats?.uptimeSecs ?? 0)}</dd>
          <dt>实时连接</dt>
          <dd style={{ color: connected ? 'var(--ok)' : 'var(--err)' }}>
            {connected ? '已连接' : '已断开'}
          </dd>
          <dt>WebSocket 客户端</dt>
          <dd>{stats?.wsClients ?? 0}</dd>
          <dt>已加载插件</dt>
          <dd>{stats?.plugins ?? 0}</dd>
        </dl>
      </Section>
    </div>
  )
}
