import { useEffect, useState } from 'react'

import { api } from '../api/client'
import type { SearchMode, SourceInfo } from '../api/types'
import { useStore } from '../state/store'
import { Badge, Button, Field, Section, Select, toast, withToast } from '../ui'

const MODES: { value: SearchMode; label: string }[] = [
  { value: 'hybrid', label: '混合（关键词 + 模糊 + 语义）' },
  { value: 'keyword', label: '关键词（BM25 精确匹配）' },
  { value: 'fuzzy', label: '模糊（容错拼写）' },
  { value: 'semantic', label: '语义（向量近邻）' },
]

const DENSITIES = [
  { value: 'compact', label: '紧凑（26px 行高）' },
  { value: 'comfortable', label: '宽松（32px 行高）' },
]

export function SettingsPage() {
  const server = useStore((s) => s.server)
  const stats = useStore((s) => s.stats)
  const mode = useStore((s) => s.mode)
  const density = useStore((s) => s.density)
  const setMode = useStore((s) => s.setMode)
  const setDensity = useStore((s) => s.setDensity)

  const [sources, setSources] = useState<SourceInfo[]>([])

  useEffect(() => {
    api.scrape.sources().then(setSources).catch(() => setSources([]))
  }, [])

  const copy = async (value: string, label: string) => {
    await navigator.clipboard.writeText(value)
    toast.success(`已复制${label}`)
  }

  return (
    <div className="page narrow">
      <Section title="检索">
        <Field label="默认搜索模式" hint="也可以在搜索框右侧随时切换。">
          <Select
            value={mode}
            options={MODES}
            onChange={(e) => setMode(e.target.value as SearchMode)}
          />
        </Field>
        <Field
          label="查询语法"
          hint="在搜索框中可用的操作符，与默认模式无关。"
        >
          <div className="syntax">
            <code>tag:综述</code>
            <code>-tag:废弃</code>
            <code>type:book</code>
            <code>author:zhang</code>
            <code>year:2020..2024</code>
            <code>&quot;精确短语&quot;</code>
          </div>
        </Field>
      </Section>

      <Section title="外观">
        <Field label="列表密度">
          <Select
            value={density}
            options={DENSITIES}
            onChange={(e) => setDensity(e.target.value)}
          />
        </Field>
      </Section>

      <Section title="快速添加">
        <Field
          label="已启用的解析源"
          hint="粘贴标识符时按特异性依次尝试；网页元数据是最后的兜底。"
        >
          <div className="chip-row tight">
            {sources.map((s) => (
              <Badge key={s.id} tone="accent" title={s.supports.join(' / ')}>
                {s.label}
              </Badge>
            ))}
            {sources.length === 0 && <span className="muted">载入中…</span>}
          </div>
        </Field>
      </Section>

      <Section title="存储">
        <Field label="数据目录" hint="文库、索引、插件与日志都在这里。">
          <div className="path-row">
            <code>{server?.dataDir ?? '—'}</code>
            <Button
              tone="ghost"
              disabled={!server}
              onClick={() => void copy(server?.dataDir ?? '', '路径')}
            >
              复制
            </Button>
          </div>
        </Field>
        <Field label="插件目录">
          <ul className="path-list">
            {(server?.pluginDirs ?? []).map((d) => (
              <li key={d}>{d}</li>
            ))}
          </ul>
        </Field>
      </Section>

      <Section title="维护">
        <div className="button-row">
          <Button
            onClick={() =>
              withToast(useStore.getState().reindex, {
                success: '索引已重建',
                failure: '重建索引失败',
              })
            }
          >
            重建搜索索引
          </Button>
          <Button
            onClick={() =>
              withToast(useStore.getState().optimize, {
                success: '数据库已优化',
                failure: '优化失败',
              })
            }
          >
            优化数据库
          </Button>
        </div>
        <p className="note">
          索引与向量都是派生数据，随时可以重建；条目本身不受影响。
        </p>
      </Section>

      <Section title="关于">
        <dl className="kv">
          <dt>Yinkote</dt>
          <dd>{server?.version ?? '—'}</dd>
          <dt>嵌入提供方</dt>
          <dd>{stats?.search.provider ?? '—'}</dd>
          <dt>许可证</dt>
          <dd>AGPL-3.0-or-later</dd>
        </dl>
      </Section>
    </div>
  )
}
