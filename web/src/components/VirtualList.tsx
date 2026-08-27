/** One scrolling list, used by every surface that shows rows.
 *
 *  Two things were wrong before this existed. The item table virtualised and
 *  the other lists did not, so downloads and files rendered every row on every
 *  poll and felt heavy for no reason a user could see. And the header was a
 *  sibling of the scrolling body, so scrolling sideways moved the rows out
 *  from under their own column names.
 *
 *  Both are fixed by shape rather than by code: the header lives *inside* the
 *  scroller and sticks to its top. Sideways scrolling then moves header and
 *  rows together because they are the same scroller, and downwards scrolling
 *  pins the header because that is what `sticky` means. No scroll handler, no
 *  synchronising, nothing to fall out of step.
 */
import { useVirtualizer } from '@tanstack/react-virtual'
import { useEffect, useRef, type ReactNode } from 'react'

import { shouldScroll } from '../lib/follow'

export interface VirtualListProps<T> {
  rows: T[]
  /** Stable identity per row; a list keyed by index re-renders everything. */
  keyOf: (row: T, index: number) => string
  /** Rendered inside the scroller, above the rows, pinned while scrolling. */
  header?: ReactNode
  /** The row's contents. Position and height belong to the list. */
  children: (row: T, index: number) => ReactNode
  /** Row height in pixels, when the rows are uniform. */
  rowHeight?: number
  /**
   * Measure each row instead of assuming `rowHeight`.
   *
   * For lists whose rows differ wildly — a one-line answer beside a tool
   * trace with four hundred lines of JSON. `rowHeight` is then only the
   * first guess, corrected once the row is on screen.
   */
  dynamic?: boolean
  /** Widest the content gets, so columns may overflow and scroll sideways. */
  minWidth?: string | number
  /** Called when the end comes into view. */
  onEndReached?: () => void
  /** The first row currently on screen, for a scrollbar or a rail. */
  onVisibleChange?: (index: number) => void
  /** Keeps this row in view — the keyboard cursor, usually. */
  scrollTo?: number
  className?: string
  /** Shown instead of the rows when there are none. */
  empty?: ReactNode
}

/** How many rows past the viewport to keep mounted. */
const OVERSCAN = 16

/** How close to the end counts as "reached it". */
const NEAR_END = 24

/** Fewer, because a measured row may be very tall and mounting it is not free. */
const DYNAMIC_OVERSCAN = 4

export function VirtualList<T>({
  rows,
  keyOf,
  header,
  children,
  rowHeight = 26,
  dynamic = false,
  minWidth,
  onEndReached,
  onVisibleChange,
  scrollTo,
  className,
  empty,
}: VirtualListProps<T>) {
  const scroller = useRef<HTMLDivElement>(null)
  const virtual = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => rowHeight,
    overscan: dynamic ? DYNAMIC_OVERSCAN : OVERSCAN,
    // Measuring costs a layout read per row, so it is only done where the
    // heights genuinely differ.
    measureElement: dynamic
      ? (el) => el.getBoundingClientRect().height
      : undefined,
  })

  // What has already been scrolled to, so that growing the list does not
  // re-run a request that was honoured pages ago. See `shouldScroll`.
  const honoured = useRef<number | null>(null)
  useEffect(() => {
    if (scrollTo === undefined) return
    if (!shouldScroll(scrollTo, honoured.current, rows.length)) return
    honoured.current = scrollTo
    virtual.scrollToIndex(scrollTo, { align: 'auto' })
  }, [scrollTo, rows.length, virtual])

  // Driven by the virtualiser rather than a scroll handler, so it also fires
  // when the cursor is walked to the end with the keyboard.
  const items = virtual.getVirtualItems()
  const last = items[items.length - 1]?.index ?? 0
  useEffect(() => {
    if (rows.length && last >= rows.length - NEAR_END) onEndReached?.()
  }, [last, rows.length, onEndReached])

  const first = items[0]?.index ?? 0
  useEffect(() => {
    onVisibleChange?.(first)
  }, [first, onVisibleChange])

  return (
    <div ref={scroller} className={`vlist${className ? ` ${className}` : ''}`}>
      <div className="vlist-inner" style={minWidth ? { minWidth } : undefined}>
        {header}
        {rows.length === 0 && empty}
        <div className="vlist-rows" style={{ height: virtual.getTotalSize() }}>
          {items.map((v) => {
            const row = rows[v.index]
            if (row === undefined) return null
            return (
              // The list owns placement, the caller owns content: a caller
              // that had to position its own rows would be reimplementing the
              // virtualiser badly.
              <div
                key={keyOf(row, v.index)}
                className="vlist-slot"
                data-index={v.index}
                ref={dynamic ? virtual.measureElement : undefined}
                style={
                  dynamic
                    ? { transform: `translateY(${v.start}px)` }
                    : { transform: `translateY(${v.start}px)`, height: v.size }
                }
              >
                {children(row, v.index)}
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}
