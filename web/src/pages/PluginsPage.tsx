import { useMemo, useState } from 'react'

import { api } from '../api/client'
import type { PluginStatus } from '../api/types'
import { useStore } from '../state/store'
import { Badge, Button, Empty, Field, Section, Textarea, toast, withToast } from '../ui'
import { useT } from '../i18n'

/** Full-page plugin manager: inventory, health, permissions and a console for
 *  calling a plugin's own methods while developing it. */
/** Every contribution a plugin makes, rendered the same way.
 *
 *  Listing only metadata sources meant a plugin that contributed a badge or an
 *  importer looked like it contributed nothing at all. */
function contributionChips(plugin: PluginStatus) {
  const c = plugin.contributions
  return [
    ...c.metadataSources.map((s) => ({ key: `s:${s.id}`, label: s.label, kind: 'plugins.kind.source' as const })),
    ...c.importers.map((f) => ({ key: `i:${f.id}`, label: f.label, kind: 'plugins.kind.importer' as const })),
    ...c.exporters.map((f) => ({ key: `e:${f.id}`, label: f.label, kind: 'plugins.kind.exporter' as const })),
    ...c.itemActions.map((a) => ({ key: `a:${a.id}`, label: a.label, kind: 'plugins.kind.action' as const })),
    ...(c.badges ?? []).map((b) => ({ key: `b:${b.id}`, label: b.label, kind: 'plugins.kind.badge' as const })),
  ]
}

/** Anything broken first, then anything off, then the rest by name. */
const STATE_RANK: Record<string, number> = { failed: 0, disabled: 1 }

export function PluginsPage() {
  const t = useT()
  const allPlugins = useStore((s) => s.plugins)
  const plugins = useMemo(
    () =>
      [...allPlugins].sort(
        (a, b) =>
          (STATE_RANK[a.state] ?? 2) - (STATE_RANK[b.state] ?? 2) || a.name.localeCompare(b.name),
      ),
    [allPlugins],
  )
  const server = useStore((s) => s.server)
  const setEnabled = useStore((s) => s.setPluginEnabled)
  const reload = useStore((s) => s.reloadPlugins)

  const [target, setTarget] = useState('')
  const [request, setRequest] = useState('{\n  "method": "initialize",\n  "params": {}\n}')
  const [response, setResponse] = useState('')
  const [busy, setBusy] = useState(false)

  const call = async () => {
    setBusy(true)
    try {
      const body = JSON.parse(request) as { method: string; params?: unknown }
      const result = await api.plugins.call(target, body.method, body.params ?? {})
      setResponse(JSON.stringify(result, null, 2))
    } catch (error) {
      setResponse(String(error instanceof Error ? error.message : error))
      toast.fromError(t('toast.callFailed'), error)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="page">
      <Section
        title={t('plugins.installed', { count: plugins.length })}
        action={
          <Button
            onClick={() =>
              withToast(reload, {
                success: t('toast.rescanned'),
                failure: t('toast.rescanFailed'),
              })
            }
          >
            {t('plugins.rescan')}
          </Button>
        }
      >
        {plugins.length === 0 ? (
          <Empty>
            {t('plugins.none')}
            <ul className="path-list">
              {(server?.pluginDirs ?? []).map((d) => (
                <li key={d}>{d}</li>
              ))}
            </ul>
            {t('plugins.manifestHint')}
          </Empty>
        ) : (
          <div className="plugin-grid">
            {plugins.map((p) => (
              <article className="plugin-card" key={p.id} data-state={p.state}>
                <header>
                  <span className="name">{p.name}</span>
                  <span className="version">{p.version}</span>
                  <span className="state" data-s={p.state}>
                    {p.state}
                  </span>
                </header>

                {p.description && <p className="desc">{p.description}</p>}
                {p.error && <p className="desc error">{p.error}</p>}

                <dl className="kv compact">
                  <dt>{t('plugins.id')}</dt>
                  <dd>{p.id}</dd>
                  <dt>{t('plugins.calls')}</dt>
                  <dd>
                    {p.calls} / {p.failures}
                  </dd>
                  <dt>{t('plugins.latency')}</dt>
                  <dd>{p.avgLatencyMs.toFixed(0)}ms</dd>
                  <dt>{t('plugins.source')}</dt>
                  <dd className="path">{p.source}</dd>
                </dl>

                {p.permissions.length > 0 && (
                  <div className="chip-row tight">
                    {p.permissions.map((perm) => (
                      <Badge key={perm} tone="warn" title={t('plugins.permission')}>
                        {perm}
                      </Badge>
                    ))}
                  </div>
                )}
                {p.hooks.length > 0 && (
                  <div className="chip-row tight">
                    {p.hooks.map((h) => (
                      <Badge key={h} title={t('plugins.hook')}>
                        {h}
                      </Badge>
                    ))}
                  </div>
                )}
                {contributionChips(p).length > 0 && (
                  <div className="chip-row tight">
                    {contributionChips(p).map((c) => (
                      <Badge key={c.key} tone="accent" title={t(c.kind)}>
                        {c.label}
                      </Badge>
                    ))}
                  </div>
                )}

                <footer>
                  <Button
                    tone={p.state === 'disabled' ? 'primary' : 'default'}
                    onClick={() =>
                      withToast(() => setEnabled(p.id, p.state === 'disabled'), {
                        success:
                          p.state === 'disabled'
                            ? t('toast.pluginEnabled', { name: p.name })
                            : t('toast.pluginDisabled', { name: p.name }),
                        failure: t('toast.pluginFailed'),
                      })
                    }
                  >
                    {p.state === 'disabled' ? t('plugins.enable') : t('plugins.disable')}
                  </Button>
                  <Button
                    tone="ghost"
                    onClick={() => {
                      setTarget(p.id)
                      document.getElementById('plugin-console')?.scrollIntoView({ behavior: 'smooth' })
                    }}
                  >
                    {t('plugins.call')}
                  </Button>
                </footer>
              </article>
            ))}
          </div>
        )}
      </Section>

      <Section
        title={t('plugins.console')}
        action={<span className="muted">{t('plugins.consoleHint')}</span>}
      >
        <div id="plugin-console" className="console">
          <Field label={t('plugins.plugin')}>
            <select
              className="ctl"
              value={target}
              onChange={(e) => setTarget(e.target.value)}
            >
              <option value="">{t('plugins.choose')}</option>
              {plugins.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name} ({p.id})
                </option>
              ))}
            </select>
          </Field>
          <Field label={t('plugins.request')}>
            <Textarea rows={7} value={request} onChange={(e) => setRequest(e.target.value)} />
          </Field>
          <Button tone="primary" disabled={!target || busy} onClick={() => void call()}>
            {busy ? t('plugins.sending') : t('plugins.send')}
          </Button>
          {response && (
            <Field label={t('plugins.response')}>
              <pre className="code">{response}</pre>
            </Field>
          )}
        </div>
      </Section>
    </div>
  )
}
