import { useCallback, useEffect, useState } from 'react'

import { api } from '../api/client'
import type { CitationList, Conversation, Item } from '../api/types'
import { creatorName } from '../lib/format'
import { tagColour } from '../lib/tags'
import { useStore } from '../state/store'
import { useSchemaLabel, useT } from '../i18n'
import { useDebounced } from '../lib/useDebounced'
import { toast } from '../ui'

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
  const t = useT()
  const patchItem = useStore((s) => s.patchItem)
  // The stored colours live with the tag list, not on the item's tags — an
  // item carries names, and a name is what a colour belongs to.
  const tagColours = useStore((s) => s.tagColours)
  const [draft, setDraft] = useState('')

  const setTags = (tags: { tag: string; type?: number }[]) =>
    void patchItem(item.key, { tags })

  return (
    <>
      <dt>{t('detail.tags')}</dt>
      <dd>
        <div className="chip-row">
          {item.tags.map((tag) => (
            <span
              key={tag.tag}
              className="chip"
              data-colour={tagColour(tag.tag, tagColours[tag.tag])}
              title={tag.type === 1 ? t('detail.tagAuto') : t('detail.tagManual')}
            >
              {tag.tag}
              <button
                onClick={() => setTags(item.tags.filter((x) => x.tag !== tag.tag))}
                title={t('detail.remove')}
              >
                ×
              </button>
            </span>
          ))}
          <input
            value={draft}
            placeholder={t('detail.addTag')}
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
  const t = useT()
  const label = useSchemaLabel()
  const schema = useStore((s) => s.schema)
  const items = useStore((s) => s.items)
  const selected = useStore((s) => s.selected)
  const patchItem = useStore((s) => s.patchItem)
  const collections = useStore((s) => s.collections)
  const addSelectedToCollection = useStore((s) => s.addSelectedToCollection)

  const detached = useStore((s) => s.detached)

  // The list is the usual source, but not the only one: a graph neighbour is
  // shown here without ever appearing in the table behind it. The key check is
  // what makes a stale detached item harmless.
  const item =
    items.find((i) => i.key === selected[0]) ??
    (detached?.key === selected[0] ? detached : undefined)

  if (!item) {
    return (
      <aside className="pane">
        <div className="pane-header">{t('detail.title')}</div>
        <div className="empty">
          {selected.length > 1
            ? t('detail.multiple', { count: selected.length })
            : t('detail.none')}
        </div>
      </aside>
    )
  }

  const typeDef = schema?.itemTypes.find((t) => t.type === item.itemType)
  const fields = typeDef?.fields ?? ['title', 'date', 'abstractNote']

  return (
    <aside className="pane">
      <div className="pane-header">
        {t('detail.title')}
        <span className="spacer" />
        <span style={{ fontFamily: 'var(--mono)' }}>{item.key}</span>
        <span>v{item.version}</span>
      </div>

      <div className="detail">
        <div className="detail-title">{String(item.title ?? t('detail.untitled'))}</div>

        <dl className="field-grid">
          <dt>{t('detail.type')}</dt>
          <dd>
            <select
              value={item.itemType}
              onChange={(e) => void patchItem(item.key, { itemType: e.target.value })}
            >
              {schema?.itemTypes
                .filter((d) => !d.internal)
                .map((d) => (
                  <option key={d.type} value={d.type}>
                    {label(d, d.type)}
                  </option>
                ))}
            </select>
          </dd>

          <dt>{t('detail.authors')}</dt>
          <dd>
            <div className="chip-row">
              {item.creators.map((c, i) => (
                <span key={i} className="chip">
                  {creatorName(c)}
                  <button
                    title={t('detail.remove')}
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
                placeholder={t('detail.addAuthor')}
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
                label={label(schema?.fields[f], f)}
              />
            ))}

          <dt>{t('detail.collections')}</dt>
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
                <option value="">{t('detail.addCollection')}</option>
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

          <ItemReferences itemKey={item.key} />
          <ItemConversations itemKey={item.key} />
        </dl>
      </div>
    </aside>
  )
}

/** What has already been asked about this paper.
 *
 *  Asked from the paper rather than from the chat list: standing on something
 *  you are reading, "what did I already work out about this" is a question the
 *  library should answer without making you remember which thread it was in.
 */
function ItemConversations({ itemKey: selected }: { itemKey: string }) {
  const t = useT()
  const itemKey = useDebounced(selected)
  const library = useStore((s) => s.library)
  const openConversation = useStore((s) => s.openConversation)
  const askAbout = useStore((s) => s.askAbout)
  const [found, setFound] = useState<Conversation[]>([])

  useEffect(() => {
    let live = true
    void api.conversations
      .aboutItem(library, itemKey)
      .then((r) => {
        if (live) setFound(r.conversations)
      })
      .catch(() => {
        if (live) setFound([])
      })
    return () => {
      live = false
    }
  }, [library, itemKey])

  return (
    <>
      <dt>{t('item.conversations')}</dt>
      <dd>
        <div className="chip-row">
          {found.map((c) => (
            <button
              key={c.key}
              className="chip"
              onClick={() => void openConversation(c.key)}
              title={c.title}
            >
              {c.title || c.key}
            </button>
          ))}
          {!found.length && <span className="dim">{t('item.conversationsNone')}</span>}
          <button className="chip" onClick={() => void askAbout(itemKey)}>
            {t('item.askAboutThis')}
          </button>
        </div>
      </dd>
    </>
  )
}

/** What this paper stands on, and what stands on it.
 *
 *  The citation data has been stored and used by the graph since it arrived,
 *  with nowhere to read it plainly — so the most direct question anybody has
 *  of it ("what does this cite, and which of those do I have?") had no answer
 *  in the interface.
 *
 *  A cited work the library holds is a link; one it does not is the label the
 *  publisher printed. Neither case is special, which is the point.
 */
function ItemReferences({ itemKey: selected }: { itemKey: string }) {
  const t = useT()
  // Debounced: arrow-keying down a list must not fetch a bibliography for
  // every row it passes through.
  const itemKey = useDebounced(selected)
  const library = useStore((s) => s.library)
  const openReader = useStore((s) => s.openReader)
  const [list, setList] = useState<CitationList | null>(null)
  const [fetching, setFetching] = useState(false)

  const load = useCallback(() => {
    let live = true
    void api.references
      .list(library, itemKey)
      .then((r) => {
        if (live) setList(r)
      })
      .catch(() => {
        if (live) setList(null)
      })
    return () => {
      live = false
    }
  }, [library, itemKey])

  useEffect(load, [load])

  const fetchRefs = async () => {
    setFetching(true)
    try {
      await api.references.fetch(library, itemKey)
      load()
    } catch (e) {
      toast.fromError(t('detail.referencesFailed'), e)
    } finally {
      setFetching(false)
    }
  }

  const cites = list?.cites ?? []

  return (
    <>
      <dt>{t('detail.references')}</dt>
      <dd>
        {cites.length === 0 ? (
          <div className="chip-row">
            <span className="dim">{t('detail.referencesNone')}</span>
            <button className="chip" disabled={fetching} onClick={() => void fetchRefs()}>
              {fetching ? t('detail.referencesFetching') : t('detail.referencesFetch')}
            </button>
          </div>
        ) : (
          <div className="reference-list">
            <span className="dim">
              {t('detail.referencesHeld', { held: list?.resolved ?? 0, total: cites.length })}
            </span>
            {cites.map((c) => (
              <div key={c.position} className="reference-row" title={c.label}>
                <span className="dim mono">{c.position + 1}</span>
                {c.key ? (
                  <button className="link" onClick={() => openReader(c.key!)}>
                    {c.label || c.fingerprint}
                  </button>
                ) : (
                  <span className="reference-absent">{c.label || c.fingerprint}</span>
                )}
                {c.year && <span className="dim">{c.year}</span>}
              </div>
            ))}
          </div>
        )}
      </dd>
    </>
  )
}
