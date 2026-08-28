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
  const rows = flatten(nodes)

  // The heading being read is the last one at or before the current page. A
  // reader wants to know where they are, and the outline is the only thing on
  // screen that can say it in words.
  let active = -1
  rows.forEach((row, i) => {
    if (row.page !== null && row.page <= current) active = i
  })

  return (
    <div className="outline" aria-label={t('reader.outline')}>
      {rows.map((row, i) => (
        <button
          key={`${i}-${row.title}`}
          className="outline-row"
          style={{ paddingLeft: `${6 + Math.min(row.depth, 4) * 10}px` }}
          data-active={i === active}
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
      ))}
    </div>
  )
}
