import { useState } from 'react'

import { useT } from '../i18n'
import { allColumns, badgeColumn } from '../lib/columns'
import { useStore } from '../state/store'
import { ColumnPicker } from '../components/ColumnPicker'
import { Icon } from '../ui'

/**
 * What the status bar says depends on what is in front.
 *
 * The row count and the column controls belong to the library, not to the
 * window, so they live here and appear only when a library tab is showing —
 * a table's chrome sitting under a PDF was describing something the reader
 * could not see.
 */
/** Shows and hides the detail pane. Shared by every surface that has one. */
function DetailToggle() {
  const t = useT()
  const detailOpen = useStore((s) => s.detailOpen)
  const toggleDetail = useStore((s) => s.toggleDetail)
  return (
    <button
      className="icon-btn"
      data-active={detailOpen}
      title={detailOpen ? t('detail.hide') : t('detail.show')}
      onClick={() => toggleDetail()}
    >
      <Icon.Panel size={12} />
    </button>
  )
}

export function LibraryFooter() {
  const t = useT()
  const items = useStore((s) => s.items)
  const total = useStore((s) => s.total)
  const approximate = useStore((s) => s.approximate)
  const loading = useStore((s) => s.loading)
  const loadingMore = useStore((s) => s.loadingMore)
  const badgeDefs = useStore((s) => s.badgeDefs)
  const [picking, setPicking] = useState(false)

  const available = allColumns(badgeDefs.map((b) => badgeColumn(b)))
  const label = (c: { id: string; labelKey: Parameters<typeof t>[0] }) =>
    c.id.startsWith('badge:')
      ? (badgeDefs.find((b) => `badge:${b.pluginId}:${b.id}` === c.id)?.label ?? c.id)
      : t(c.labelKey)

  return (
    <>
      {/* A ranked search knows it found "at least" this many; a browse counts
          exactly. Rendering both the same way would read as precision that a
          search does not have. */}
      <span>
        {t(approximate ? 'table.countApprox' : 'table.count', {
          shown: items.length,
          total,
        })}
      </span>
      {(loading || loadingMore) && <span className="dim">{t('table.loading')}</span>}
      <span className="spacer" />

      <span className="column-anchor">
        <button
          className="icon-btn"
          data-active={picking}
          title={t('table.columns')}
          onClick={() => setPicking((p) => !p)}
        >
          <Icon.Columns size={12} />
        </button>
        {picking && (
          <ColumnPicker
            available={available}
            label={label}
            onClose={() => setPicking(false)}
          />
        )}
      </span>
      <DetailToggle />
    </>
  )
}

export function CollectionsFooter() {
  const t = useT()
  const collections = useStore((s) => s.collections)
  const smart = useStore((s) => s.smartCollections)
  return (
    <span>
      {t('collections.footer', { plain: collections.length, smart: smart.length })}
    </span>
  )
}

export function ChatFooter() {
  const t = useT()
  const agent = useStore((s) => s.agent)
  const messages = useStore((s) => s.messages)
  return (
    <>
      <span>{t('chat.turns', { count: messages.length })}</span>
      <span className="spacer" />
      <span className="dim">{agent?.configured ? agent.model : t('summary.needsModel')}</span>
    </>
  )
}

export function GraphFooter() {
  const t = useT()
  const nodes = useStore((s) => s.graphSize.nodes)
  const edges = useStore((s) => s.graphSize.edges)
  return (
    <>
      <span>{t('graph.footer', { nodes, edges })}</span>
      <span className="spacer" />
      <DetailToggle />
    </>
  )
}

export function GapsFooter() {
  const t = useT()
  const count = useStore((s) => s.gapCount)
  return <span>{t('gaps.footer', { count })}</span>
}

export function ReaderFooter() {
  return (
    <>
      <span className="spacer" />
      <DetailToggle />
    </>
  )
}
