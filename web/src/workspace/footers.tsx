import { useT } from '../i18n'
import { useStore } from '../state/store'

/**
 * What the status bar says depends on what is in front.
 *
 * Counts belong to the surface in front. Controls live in the top toolbar
 * and table headers, so no footer owns a second version of those controls.
 */
export function LibraryFooter() {
  const t = useT()
  const items = useStore((s) => s.items)
  const total = useStore((s) => s.total)
  const approximate = useStore((s) => s.approximate)
  const ranked = useStore((s) => s.ranked)
  const loading = useStore((s) => s.loading)
  const loadingMore = useStore((s) => s.loadingMore)
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
    </>
  )
}

export function GapsFooter() {
  const t = useT()
  const count = useStore((s) => s.gapCount)
  return <span>{t('gaps.footer', { count })}</span>
}

export function ChatsFooter() {
  const t = useT()
  const conversations = useStore((s) => s.conversations)
  const turns = conversations.reduce((n, c) => n + c.messageCount, 0)
  return (
    <>
      <span>{t('chats.footer', { count: conversations.length, turns })}</span>
      <span className="spacer" />
    </>
  )
}
