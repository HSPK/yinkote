import { useEffect, useState } from 'react'

import type { Item } from '../api/types'
import { creatorName } from '../lib/format'
import { useStore } from '../state/store'

/** Fields worth a multi-line editor. */
const LONG_FIELDS = new Set(['abstractNote', 'extra', 'note'])

function FieldEditor({ item, field, label }: { item: Item; field: string; label: string }) {
  const patchItem = useStore((s) => s.patchItem)
  const stored = String(item[field] ?? '')
  const [value, setValue] = useState(stored)

  // Re-sync when the selection changes or the server sends an update.
  useEffect(() => setValue(stored), [stored, item.key])

  const commit = () => {
    if (value === stored) return
    void patchItem(item.key, { fields: { [field]: value === '' ? null : value } })
  }

  return (
    <>
      <dt title={field}>{label}</dt>
      <dd>
        {LONG_FIELDS.has(field) ? (
          <textarea
            rows={field === 'abstractNote' ? 5 : 2}
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onBlur={commit}
          />
        ) : (
          <input
            value={value}
            spellCheck={false}
            onChange={(e) => setValue(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === 'Enter') e.currentTarget.blur()
              if (e.key === 'Escape') {
                setValue(stored)
                e.currentTarget.blur()
              }
            }}
          />
        )}
      </dd>
    </>
  )
}

function TagEditor({ item }: { item: Item }) {
  const patchItem = useStore((s) => s.patchItem)
  const [draft, setDraft] = useState('')

  const setTags = (tags: { tag: string; type?: number }[]) =>
    void patchItem(item.key, { tags })

  return (
    <>
      <dt>标签</dt>
      <dd>
        <div className="chip-row">
          {item.tags.map((t) => (
            <span key={t.tag} className="chip" title={t.type === 1 ? '自动标签' : '手动标签'}>
              {t.tag}
              <button
                onClick={() => setTags(item.tags.filter((x) => x.tag !== t.tag))}
                title="移除"
              >
                ×
              </button>
            </span>
          ))}
          <input
            value={draft}
            placeholder="+ 标签"
            spellCheck={false}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key !== 'Enter') return
              const name = draft.trim()
              if (name && !item.tags.some((t) => t.tag === name)) {
                setTags([...item.tags, { tag: name, type: 0 }])
              }
              setDraft('')
            }}
          />
        </div>
      </dd>
    </>
  )
}

export function DetailPanel() {
  const schema = useStore((s) => s.schema)
  const items = useStore((s) => s.items)
  const selected = useStore((s) => s.selected)
  const patchItem = useStore((s) => s.patchItem)
  const collections = useStore((s) => s.collections)
  const addSelectedToCollection = useStore((s) => s.addSelectedToCollection)

  const item = items.find((i) => i.key === selected[0])

  if (!item) {
    return (
      <aside className="pane">
        <div className="pane-header">详情</div>
        <div className="empty">
          {selected.length > 1 ? `已选中 ${selected.length} 条` : '未选中条目'}
        </div>
      </aside>
    )
  }

  const typeDef = schema?.itemTypes.find((t) => t.type === item.itemType)
  const fields = typeDef?.fields ?? ['title', 'date', 'abstractNote']

  return (
    <aside className="pane">
      <div className="pane-header">
        详情
        <span className="spacer" />
        <span style={{ fontFamily: 'var(--mono)' }}>{item.key}</span>
        <span>v{item.version}</span>
      </div>

      <div className="detail">
        <div className="detail-title">{String(item.title ?? '(无标题)')}</div>

        <dl className="field-grid">
          <dt>类型</dt>
          <dd>
            <select
              value={item.itemType}
              onChange={(e) => void patchItem(item.key, { itemType: e.target.value })}
            >
              {schema?.itemTypes
                .filter((t) => !t.internal)
                .map((t) => (
                  <option key={t.type} value={t.type}>
                    {t.label}
                  </option>
                ))}
            </select>
          </dd>

          <dt>作者</dt>
          <dd>
            <div className="chip-row">
              {item.creators.map((c, i) => (
                <span key={i} className="chip">
                  {creatorName(c)}
                  <button
                    title="移除"
                    onClick={() =>
                      void patchItem(item.key, {
                        creators: item.creators.filter((_, j) => j !== i),
                      })
                    }
                  >
                    ×
                  </button>
                </span>
              ))}
              <input
                placeholder="+ 作者"
                spellCheck={false}
                onKeyDown={(e) => {
                  if (e.key !== 'Enter') return
                  const raw = e.currentTarget.value.trim()
                  if (!raw) return
                  // "Wei Zhang" splits; a single token (or CJK) stays one field.
                  const parts = raw.split(/\s+/)
                  const creator =
                    parts.length > 1
                      ? { creatorType: 'author', firstName: parts.slice(0, -1).join(' '), lastName: parts.at(-1) }
                      : { creatorType: 'author', name: raw }
                  void patchItem(item.key, { creators: [...item.creators, creator] })
                  e.currentTarget.value = ''
                }}
              />
            </div>
          </dd>

          <TagEditor item={item} />

          {fields
            .filter((f) => f !== 'title')
            .map((f) => (
              <FieldEditor
                key={f}
                item={item}
                field={f}
                label={schema?.fields[f]?.label ?? f}
              />
            ))}

          <dt>收藏夹</dt>
          <dd>
            <div className="chip-row">
              {item.collections.map((k) => (
                <span key={k} className="chip">
                  {collections.find((c) => c.key === k)?.name ?? k}
                </span>
              ))}
              <select
                value=""
                onChange={(e) => {
                  if (e.target.value) void addSelectedToCollection(e.target.value)
                }}
              >
                <option value="">+ 加入…</option>
                {collections
                  .filter((c) => !item.collections.includes(c.key))
                  .map((c) => (
                    <option key={c.key} value={c.key}>
                      {c.name}
                    </option>
                  ))}
              </select>
            </div>
          </dd>
        </dl>
      </div>
    </aside>
  )
}
