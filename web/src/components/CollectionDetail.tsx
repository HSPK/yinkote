import { useMemo } from 'react'

import { useT } from '../i18n'
import { collectionColour, collectionIcon } from '../lib/collections'
import { compact } from '../lib/format'
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
          {chosen.name}
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
