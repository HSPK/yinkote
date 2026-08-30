import { useState } from 'react'

import { useT } from '../i18n'
import { COLLECTION_COLUMNS, allColumns, badgeColumn, type ColumnDef, type TableId } from '../lib/columns'
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

/**
 * The button that opens a table's column picker.
 *
 * Shared, because the collection browser wants exactly this and a second copy
 * would be a second place to fix the stale-popover bug ColumnPicker documents.
 */
function ColumnButton({
  table,
  available,
  label,
}: {
  table: TableId
  available: ColumnDef[]
  label: (c: ColumnDef) => string
}) {
  const t = useT()
  const [picking, setPicking] = useState(false)
  return (
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
          table={table}
          available={available}
          label={label}
          onClose={() => setPicking(false)}
        />
      )}
    </span>
  )
}

export function LibraryFooter() {
  const t = useT()
  const items = useStore((s) => s.items)
  const total = useStore((s) => s.total)
  const approximate = useStore((s) => s.approximate)
  const ranked = useStore((s) => s.ranked)
  const loading = useStore((s) => s.loading)
  const loadingMore = useStore((s) => s.loadingMore)
  const badgeDefs = useStore((s) => s.badgeDefs)

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
      {/* Say what the order is, since the column header can no longer. */}
      {ranked && <span className="dim" title={t('table.rankedHint')}>{t('table.ranked')}</span>}
      {(loading || loadingMore) && <span className="dim">{t('table.loading')}</span>}
      <span className="spacer" />

      <ColumnButton table="items" available={available} label={label} />
      <DetailToggle />
    </>
  )
}

export function CollectionsFooter() {
  const t = useT()
  const collections = useStore((s) => s.collections)
  const smart = useStore((s) => s.smartCollections)
  return (
    <>
      <span>
        {t('collections.footer', { plain: collections.length, smart: smart.length })}
      </span>
      <span className="spacer" />
      <ColumnButton
        table="collections"
        available={COLLECTION_COLUMNS}
        label={(c) => t(c.labelKey)}
      />
      <DetailToggle />
    </>
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

export function ChatsFooter() {
  const t = useT()
  const conversations = useStore((s) => s.conversations)
  const turns = conversations.reduce((n, c) => n + c.messageCount, 0)
  return (
    <>
      <span>{t('chats.footer', { count: conversations.length, turns })}</span>
      <span className="spacer" />
      <DetailToggle />
    </>
  )
}
