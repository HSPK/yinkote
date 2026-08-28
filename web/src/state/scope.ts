/** Per-tab library state.
 *
 *  Two library tabs should be able to show different collections, with their
 *  own search, sort and selection — that is the whole point of tabs, and it is
 *  what makes "search means something different here" true rather than a
 *  special case.
 *
 *  The active scope is *projected* onto the store's flat fields, so components
 *  keep reading `items` and `query` directly and only this module knows about
 *  the switch. Saving and restoring happens in exactly one place, because a
 *  scope that is sometimes saved is worse than no scopes at all.
 */
import type { Item, SearchMode } from '../api/types'

export type View = 'library' | 'trash' | 'collection' | 'smart'

export interface Scope {
  view: View
  collection: string | null
  query: string
  mode: SearchMode
  sort: string
  direction: 'asc' | 'desc'
  activeTags: string[]
  typeFilter: string[]
  items: Item[]
  total: number
  /** Whether `total` is a floor. A ranked search scores a bounded pool, so it
   *  knows it found "at least" this many; a browse counts exactly. */
  approximate: boolean
  /** Whether the rows are in relevance order rather than the asked-for one.
   *  A ranked search scores a bounded pool and returns it best-first, so it
   *  cannot honour a column sort — and the header must not claim it did. */
  ranked: boolean
  loading: boolean
  loadingMore: boolean
  tookMs: number
  selected: string[]
  cursor: number
  /** Where a shift-selection measures from. */
  anchor: number
}

/** What a view falls back to when it has no opinion of its own. */
const DEFAULT_SORT = 'dateModified'
const DEFAULT_DIRECTION = 'desc' as const

export function emptyScope(patch: Partial<Scope> = {}): Scope {
  return {
    view: 'library',
    collection: null,
    query: '',
    mode: 'keyword',
    sort: DEFAULT_SORT,
    direction: DEFAULT_DIRECTION,
    activeTags: [],
    typeFilter: [],
    items: [],
    total: 0,
    approximate: false,
    ranked: false,
    loading: false,
    loadingMore: false,
    tookMs: 0,
    selected: [],
    cursor: 0,
    anchor: 0,
    ...patch,
  }
}

/** The fields a tab owns. Derived from a value so the two cannot drift. */
export const SCOPE_KEYS = Object.keys(emptyScope()) as (keyof Scope)[]

/** Lift the current flat fields out of the store. */
export function captureScope(state: Scope): Scope {
  return SCOPE_KEYS.reduce((out, key) => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    ;(out as any)[key] = state[key]
    return out
  }, {} as Scope)
}

/**
 * Select a range between two rows.
 *
 * Inclusive and direction-agnostic: dragging a shift-selection upwards must
 * select the same rows as dragging it down, which is not what a naive slice
 * gives you.
 */
export function rangeOf(keys: string[], from: number, to: number): string[] {
  const [start, end] = from <= to ? [from, to] : [to, from]
  return keys.slice(Math.max(0, start), Math.min(keys.length, end + 1))
}

/**
 * Apply a click to a selection.
 *
 * `toggle` (ctrl/cmd) adds or removes one row and moves the anchor there;
 * `range` (shift) replaces the selection with everything between the anchor
 * and the click, keeping the anchor so the range can be adjusted by shift-
 * clicking again — which is what every file manager does.
 */
export function applyClick(
  keys: string[],
  current: { selected: string[]; anchor: number },
  index: number,
  modifier: 'none' | 'toggle' | 'range',
): { selected: string[]; anchor: number; cursor: number } {
  const key = keys[index]
  if (key === undefined) return { ...current, cursor: index }

  if (modifier === 'range') {
    return { selected: rangeOf(keys, current.anchor, index), anchor: current.anchor, cursor: index }
  }
  if (modifier === 'toggle') {
    const selected = current.selected.includes(key)
      ? current.selected.filter((k) => k !== key)
      : [...current.selected, key]
    return { selected, anchor: index, cursor: index }
  }
  return { selected: [key], anchor: index, cursor: index }
}
