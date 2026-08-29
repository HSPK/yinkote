import { useState } from 'react'

/**
 *  A sidebar list long enough to hide the rows below it.
 *
 *  The sidebar is a shortcut list, not an inventory. Tags and conversations
 *  both grow without limit and both had, or needed, the same three pieces of
 *  state — how many to show, whether it is open, and how many are left — so
 *  they share one here rather than two spellings of it.
 *
 *  Collections cap differently on purpose: they have a browser to send you to,
 *  so their overflow is a link rather than an expansion. An affordance that
 *  opens a surface and one that grows in place are not the same control, and
 *  merging them would only look like sharing.
 */
export function useCapped<T>(items: T[], limit: number) {
  const [expanded, setExpanded] = useState(false)
  const shown = expanded ? items : items.slice(0, limit)
  return {
    shown,
    /** How many the list is not showing; 0 when it shows everything. */
    hidden: items.length - shown.length,
    expanded,
    /** True once expanding is meaningful — used to offer "less". */
    overflows: items.length > limit,
    expand: () => setExpanded(true),
    collapse: () => setExpanded(false),
  }
}
