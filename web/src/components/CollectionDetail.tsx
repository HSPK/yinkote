import { useMemo } from 'react'

import { useT } from '../i18n'
import { collectionColour, collectionIcon } from '../lib/collections'
import { compact, shortDate } from '../lib/format'
import { rulesFromQuery } from '../lib/query'
import { useStore } from '../state/store'
import { Badge, Button, Empty, Icon } from '../ui'

/**
 * What the detail pane shows beside the collection browser.
 *
 * The pane is not "the item inspector" — it is whatever the surface in front
 * has to say about its own selection, which for a list of collections is the
 * collection, not an item that happens to still be selected elsewhere.
 */
export function CollectionDetail() {
  const t = useT()
  const collections = useStore((s) => s.collections)
  const smartCollections = useStore((s) => s.smartCollections)
  const collection = useStore((s) => s.collection)
  const openCollectionEditor = useStore((s) => s.openCollectionEditor)
  const renameCollection = useStore((s) => s.renameCollection)
  const setCollection = (key: string) => useStore.setState({ collection: key })

  const chosen = useMemo(() => {
    const smart = smartCollections.find((c) => c.key === collection)
    if (smart) return { ...smart, smart: true, itemCount: smart.itemCount ?? 0 }
    const plain = collections.find((c) => c.key === collection)
    return plain ? { ...plain, smart: false, query: '' } : null
  }, [collection, collections, smartCollections])

  if (!chosen) {
    return (
      <aside className="pane">
        <div className="pane-header">{t('detail.title')}</div>
        <Empty>{t('collections.selectOne')}</Empty>
      </aside>
    )
  }

  const Glyph = collectionIcon(chosen.icon, chosen.smart ? 'Smart' : 'Folder')
  // Where it sits: a shelf inside another one, and what is inside it. Reading
  // "42 items" without knowing a sub-shelf contributes most of them is how
  // §3.223's disagreement went unnoticed.
  const parent = collections.find((c) => c.key === (chosen as { parentKey?: string }).parentKey)
  const children = collections.filter(
    (c) => (c as { parentKey?: string }).parentKey === chosen.key,
  )
  const rules = chosen.smart ? rulesFromQuery(chosen.query) : []

  return (
    <aside className="pane">
      <div className="pane-header">
        {t('detail.title')}
        <span className="spacer" />
        <span style={{ fontFamily: 'var(--mono)' }}>{chosen.key}</span>
      </div>

      <div className="detail">
        <div className="detail-title" data-colour={collectionColour(chosen.color)}>
          <Glyph className="glyph" size={15} />
          {/* Renaming in place: it is the commonest edit, and sending someone
              to an editor tab to change one word is a detour. */}
          <input
            className="detail-title-edit"
            defaultValue={chosen.name}
            key={chosen.key}
            spellCheck={false}
            onBlur={(e) => {
              const next = e.target.value.trim()
              if (next && next !== chosen.name) void renameCollection(chosen.key, next)
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') e.currentTarget.blur()
              if (e.key === 'Escape') {
                e.currentTarget.value = chosen.name
                e.currentTarget.blur()
              }
            }}
          />
        </div>

        <dl className="field-grid">
          <dt>{t('collections.kind')}</dt>
          <dd>
            <span className="chip-row">
              <Badge tone={chosen.smart ? 'accent' : 'default'}>
                {t(chosen.smart ? 'collections.kind.smart' : 'collections.kind.plain')}
              </Badge>
            </span>
          </dd>

          <dt>{t('collections.items')}</dt>
          <dd>
            <span className="chip-row">{compact(chosen.itemCount)}</span>
          </dd>

          {parent && (
            <>
              <dt>{t('collections.parent')}</dt>
              <dd>
                <span className="chip-row">
                  <button className="link" onClick={() => setCollection(parent.key)}>
                    {parent.name}
                  </button>
                </span>
              </dd>
            </>
          )}

          {children.length > 0 && (
            <>
              <dt>{t('collections.children', { count: children.length })}</dt>
              <dd>
                <div className="chip-row">
                  {children.map((c) => (
                    <button key={c.key} className="chip" onClick={() => setCollection(c.key)}>
                      {c.name}
                    </button>
                  ))}
                </div>
              </dd>
            </>
          )}

          <dt>{t('table.added')}</dt>
          <dd>
            <span className="chip-row">{shortDate(chosen.dateAdded) || '—'}</span>
          </dd>

          <dt>{t('table.modified')}</dt>
          <dd>
            <span className="chip-row">{shortDate(chosen.dateModified) || '—'}</span>
          </dd>

          {chosen.smart && (
            <>
              <dt>{t('smart.rules')}</dt>
              <dd>
                <div className="chip-row">
                  {rules.length === 0 && <span className="muted">{t('smart.matchesEverything')}</span>}
                  {rules.map((rule, i) => (
                    <span key={i} className="chip">
                      <span className="chip-field">{t(`search.field.${rule.field}`)}</span>
                      {t(`smart.op.${rule.op}`)} {rule.value}
                      {rule.value2 ? `–${rule.value2}` : ''}
                    </span>
                  ))}
                </div>
              </dd>

              <dt>{t('smart.compiled')}</dt>
              <dd>
                <code className="code compiled">{chosen.query}</code>
              </dd>
            </>
          )}
        </dl>

        <div className="button-row" style={{ padding: '10px 12px' }}>
          <Button onClick={() => openCollectionEditor(chosen.key)}>
            <Icon.Settings size={11} /> {t('menu.edit')}
          </Button>
        </div>
      </div>
    </aside>
  )
}
