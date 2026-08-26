import { useState } from 'react'

import { api } from '../api/client'
import { useStore } from '../state/store'
import { Badge, Button, Empty, Field, Section, Textarea, toast, withToast } from '../ui'

/** Full-page plugin manager: inventory, health, permissions and a console for
 *  calling a plugin's own methods while developing it. */
export function PluginsPage() {
  const plugins = useStore((s) => s.plugins)
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
      toast.fromError('调用失败', error)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="page">
      <Section
        title={`已安装插件 · ${plugins.length}`}
        action={
          <Button onClick={() => withToast(reload, { success: '已重新扫描', failure: '扫描失败' })}>
            重新扫描
          </Button>
        }
      >
        {plugins.length === 0 ? (
          <Empty>
            尚未安装插件。把插件目录放进下列任一位置，再点「重新扫描」：
            <ul className="path-list">
              {(server?.pluginDirs ?? []).map((d) => (
                <li key={d}>{d}</li>
              ))}
            </ul>
            每个插件目录需包含 <code>plugin.json</code>，详见仓库内 <code>plugins/README.md</code>。
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
                  <dt>标识</dt>
                  <dd>{p.id}</dd>
                  <dt>调用 / 失败</dt>
                  <dd>
                    {p.calls} / {p.failures}
                  </dd>
                  <dt>平均耗时</dt>
                  <dd>{p.avgLatencyMs.toFixed(0)}ms</dd>
                  <dt>来源</dt>
                  <dd className="path">{p.source}</dd>
                </dl>

                {p.permissions.length > 0 && (
                  <div className="chip-row tight">
                    {p.permissions.map((perm) => (
                      <Badge key={perm} tone="warn">
                        {perm}
                      </Badge>
                    ))}
                  </div>
                )}
                {p.hooks.length > 0 && (
                  <div className="chip-row tight">
                    {p.hooks.map((h) => (
                      <Badge key={h}>{h}</Badge>
                    ))}
                  </div>
                )}
                {p.contributions.metadataSources.length > 0 && (
                  <div className="chip-row tight">
                    {p.contributions.metadataSources.map((s) => (
                      <Badge key={s.id} tone="accent">
                        源: {s.label}
                      </Badge>
                    ))}
                  </div>
                )}

                <footer>
                  <Button
                    tone={p.state === 'disabled' ? 'primary' : 'default'}
                    onClick={() =>
                      withToast(() => setEnabled(p.id, p.state === 'disabled'), {
                        success: p.state === 'disabled' ? `已启用 ${p.name}` : `已停用 ${p.name}`,
                        failure: '操作失败',
                      })
                    }
                  >
                    {p.state === 'disabled' ? '启用' : '停用'}
                  </Button>
                  <Button
                    tone="ghost"
                    onClick={() => {
                      setTarget(p.id)
                      document.getElementById('plugin-console')?.scrollIntoView({ behavior: 'smooth' })
                    }}
                  >
                    调用…
                  </Button>
                </footer>
              </article>
            ))}
          </div>
        )}
      </Section>

      <Section title="调用控制台" action={<span className="muted">开发插件时直接发 JSON-RPC</span>}>
        <div id="plugin-console" className="console">
          <Field label="插件">
            <select
              className="ctl"
              value={target}
              onChange={(e) => setTarget(e.target.value)}
            >
              <option value="">选择插件…</option>
              {plugins.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}（{p.id}）
                </option>
              ))}
            </select>
          </Field>
          <Field label="请求">
            <Textarea rows={7} value={request} onChange={(e) => setRequest(e.target.value)} />
          </Field>
          <Button tone="primary" disabled={!target || busy} onClick={() => void call()}>
            {busy ? '调用中…' : '发送'}
          </Button>
          {response && (
            <Field label="响应">
              <pre className="code">{response}</pre>
            </Field>
          )}
        </div>
      </Section>
    </div>
  )
}
