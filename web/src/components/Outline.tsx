import { useT } from '../i18n'
import type { OutlineNode } from '../lib/outline'
import { flatten } from '../lib/outline'

/**
 * The document's own table of contents.
 *
 * Rendered flat with an indent rather than as nested lists: the panel is 132px
 * wide, a thesis outline is four levels deep, and nesting `<ul>`s would leave
 * the deepest headings a few characters wide. Depth is a left margin, capped,
 * so level six still reads.
 */
export function Outline({
  nodes,
  current,
  onJump,
}: {
  nodes: OutlineNode[]
  current: number
  onJump: (page: number) => void
}) {
  const t = useT()
  const [collapsed, setCollapsed] = useState<Set<OutlineNode>>(() => new Set())
  const rows = useMemo(() => flatten(nodes), [nodes])
  const visible = useMemo(() => {
    const collect = (nodes: OutlineNode[]): OutlineNode[] =>
      nodes.flatMap((node) => [node, ...(collapsed.has(node) ? [] : collect(node.children))])
    return collect(nodes)
  }, [nodes, collapsed])

  // The heading being read is the last one at or before the current page. A
  // reader wants to know where they are, and the outline is the only thing on
  // screen that can say it in words.
  let active = -1
  rows.forEach((row, i) => {
    if (row.page !== null && row.page <= current) active = i
  })

  return (
    <div className="outline" aria-label={t('reader.outline')}>
      {visible.map((row, i) => (
        <div key={`${i}-${row.title}`} className="outline-entry">
          {row.children.length > 0 && (
            <button className="outline-toggle" aria-expanded={!collapsed.has(row)}
              aria-label={t(collapsed.has(row) ? 'reader.expandHeading' : 'reader.collapseHeading', {
                title: row.title,
              })}
              onClick={() => setCollapsed((was) => {
                const next = new Set(was)
                if (next.has(row)) next.delete(row)
                else next.add(row)
                return next
              })}>
              {collapsed.has(row) ? '+' : '−'}
            </button>
          )}
        <button
          className="outline-row"
          style={{ paddingLeft: `${6 + Math.min(row.depth, 4) * 10}px` }}
          data-active={rows[active] === row}
          // A bookmark pointing at nothing is a defect in the file; the row
          // stays, because it still says what is in the document, but it does
          // not pretend to be a link.
          disabled={row.page === null}
          title={row.page === null ? row.title : `${row.title} — ${t('reader.goToPage', { page: row.page })}`}
          onClick={() => row.page !== null && onJump(row.page)}
        >
          <span className="outline-title">{row.title}</span>
          {row.page !== null && <span className="outline-page">{row.page}</span>}
        </button>
        </div>
      ))}
    </div>
  )
}
import { useMemo, useState } from 'react'
