import { useEffect } from 'react'

import { compact } from '../lib/format'
import { useStore } from '../state/store'
import { Badge, Button, Section, withToast } from '../ui'
import { useT } from '../i18n'

/** Ordered worst-first, so a failure is the first thing read. */
const PLUGIN_STATES = ['failed', 'disabled', 'starting', 'stopped', 'ready'] as const
type PluginState = (typeof PLUGIN_STATES)[number]

function Metric({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="metric">
      <span className="metric-value">{value}</span>
      <span className="metric-label">{label}</span>
      {hint && <span className="metric-hint">{hint}</span>}
    </div>
  )
}

function useDuration(): (seconds: number) => string {
  const t = useT()
  return (seconds) => {
    const d = Math.floor(seconds / 86400)
    const h = Math.floor((seconds % 86400) / 3600)
    const m = Math.floor((seconds % 3600) / 60)
    if (d) return t('statusPage.days', { d, h })
    if (h) return t('statusPage.hours', { h, m })
    return t('statusPage.minutes', { m, s: seconds % 60 })
  }
}

/** Everything about the running service in one place: corpus size, index
 *  health, connectivity and the maintenance actions that fix them. */
export function StatusPage() {
  const t = useT()
  const duration = useDuration()
  const stats = useStore((s) => s.stats)
  const server = useStore((s) => s.server)
  const tookMs = useStore((s) => s.tookMs)
  const connected = useStore((s) => s.connected)
  const reindex = useStore((s) => s.reindex)
  const optimize = useStore((s) => s.optimize)
  const reloadSidebar = useStore((s) => s.reloadSidebar)
  const plugins = useStore((s) => s.plugins)

  // The status page is the one place where a stale number is the whole problem,
  // and it only exists while its modal is open, so polling here is bounded.
  useEffect(() => {
    const timer = setInterval(() => void reloadSidebar(), 5000)
    return () => clearInterval(timer)
  }, [reloadSidebar])

  // Typed by the catalogue, so an unknown state from a newer server shows its
  // raw name instead of failing the build or rendering `[missing]`.
  const health = plugins.reduce<Record<PluginState, number>>(
    (acc, plugin) => {
      const state = plugin.state as PluginState
      acc[state] = (acc[state] ?? 0) + 1
      return acc
    },
    {} as Record<PluginState, number>,
  )

  // `stats?.search.x` guards the wrong thing: it survives no stats at all and
  // falls over on stats that arrive without their search half — which is what
  // a server mid-upgrade, or one whose search subsystem failed to start,
  // sends. This is the page somebody opens *because* something is wrong, so
  // it is the last one that may go blank on partial data.
  const embedded = stats?.search?.embedded ?? 0
  const documents = stats?.search?.documents ?? 0
  const coverage = documents === 0 ? 100 : Math.round((embedded / documents) * 100)

  return (
    <div className="page">
      <Section
        title={t('statusPage.library')}
        action={<Button tone="ghost" onClick={() => void reloadSidebar()}>{t('statusPage.refresh')}</Button>}
      >
        <div className="metrics">
          <Metric label={t('statusPage.items')} value={compact(stats?.items ?? 0)} />
          <Metric label={t('statusPage.trashed')} value={compact(stats?.trashed ?? 0)} />
          <Metric label={t('statusPage.collections')} value={String(stats?.collections ?? 0)} />
          <Metric label={t('statusPage.tags')} value={String(stats?.tags ?? 0)} />
          <Metric label={t('statusPage.version')} value={String(stats?.version ?? 0)} hint={t('statusPage.versionHint')} />
        </div>
      </Section>

      <Section
        title={t('statusPage.index')}
        action={
          <Button
            onClick={() =>
              withToast(reindex, {
              success: t('toast.reindexed'),
              failure: t('toast.reindexFailed'),
            })
            }
          >
            {t('statusPage.rebuild')}
          </Button>
        }
      >
        <div className="metrics">
          <Metric label={t('statusPage.documents')} value={compact(documents)} />
          <Metric
            label={t('statusPage.coverage')}
            value={`${coverage}%`}
            hint={`${compact(embedded)} / ${compact(documents)}`}
          />
          <Metric label={t('statusPage.dimensions')} value={String(stats?.search?.dimensions ?? 0)} />
          <Metric label={t('statusPage.provider')} value={stats?.search?.provider ?? '—'} />
          <Metric label={t('statusPage.lastQuery')} value={`${tookMs}ms`} />
        </div>
        <div
          className="meter"
          role="progressbar"
          aria-valuenow={coverage}
          title={`${compact(embedded)} / ${compact(documents)}`}
        >
          <span style={{ width: `${coverage}%` }} />
        </div>
        {coverage < 100 && <p className="note">{t('statusPage.coverageNote')}</p>}
      </Section>

      <Section
        title={t('statusPage.service')}
        action={
          <Button
            onClick={() =>
              withToast(optimize, {
              success: t('toast.optimized'),
              failure: t('toast.optimizeFailed'),
            })
            }
          >
            {t('statusPage.optimize')}
          </Button>
        }
      >
        <dl className="kv">
          <dt>Yinkote</dt>
          <dd>{server?.version ?? '—'}</dd>
          <dt>{t('statusPage.apiVersion')}</dt>
          <dd>v{server?.apiVersion ?? '—'}</dd>
          <dt>{t('statusPage.pluginProtocol')}</dt>
          <dd>v{server?.pluginApiVersion ?? '—'}</dd>
          <dt>{t('statusPage.bind')}</dt>
          <dd>{server?.bind ?? '—'}</dd>
          <dt>{t('statusPage.uptime')}</dt>
          <dd>{duration(stats?.uptimeSecs ?? 0)}</dd>
          <dt>{t('statusPage.realtime')}</dt>
          <dd style={{ color: connected ? 'var(--ok)' : 'var(--err)' }}>
            {connected ? t('status.live') : t('status.offline')}
          </dd>
          <dt>{t('statusPage.wsClients')}</dt>
          <dd>{stats?.wsClients ?? 0}</dd>
          <dt>{t('statusPage.plugins')}</dt>
          <dd className="chip-row tight">
            {PLUGIN_STATES.filter((state) => health[state]).map((state) => (
              <Badge key={state} tone={state === 'failed' ? 'warn' : 'default'}>
                {t(`plugins.state.${state}`)} {health[state]}
              </Badge>
            ))}
            {plugins.length === 0 && '0'}
          </dd>
        </dl>
      </Section>
    </div>
  )
}
