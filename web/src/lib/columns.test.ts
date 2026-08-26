import { describe, expect, it } from 'vitest'

import {
  BUILTIN_COLUMNS,
  DEFAULT_VISIBLE,
  allColumns,
  badgeColumn,
  gridTemplate,
  moveColumn,
  toggleColumn,
  visibleColumns,
} from './columns'

const badge = badgeColumn({ id: 'if', label: 'IF', pluginId: 'metrics', sortable: true })

describe('columns', () => {
  it('defaults to columns that all exist', () => {
    const ids = BUILTIN_COLUMNS.map((c) => c.id)
    for (const id of DEFAULT_VISIBLE) expect(ids).toContain(id)
  })

  it('namespaces badge columns by plugin so two plugins can both supply "if"', () => {
    const other = badgeColumn({ id: 'if', label: 'IF', pluginId: 'other' })
    expect(badge.id).not.toBe(other.id)
    expect(badge.badge).toBe('if')
  })

  it('sorts by the column id, so the server knows which plugin to ask', () => {
    expect(badge.sort).toBe(badge.id)
  })

  it('offers no sort for a badge whose plugin cannot rank its values', () => {
    // Sorting the text would put "10.5" before "9.8"; refusing is better.
    expect(badgeColumn({ id: 'x', label: 'X', pluginId: 'p' }).sort).toBeNull()
  })

  it('orders visible columns as the user arranged them, not as declared', () => {
    const got = visibleColumns(['year', 'title'], allColumns())
    expect(got.map((c) => c.id)).toEqual(['year', 'title'])
  })

  it('drops columns whose plugin is gone rather than rendering a blank', () => {
    const got = visibleColumns(['title', badge.id], allColumns())
    expect(got.map((c) => c.id)).toEqual(['title'])
  })

  it('keeps a badge column once its plugin is present', () => {
    const got = visibleColumns(['title', badge.id], allColumns([badge]))
    expect(got.map((c) => c.id)).toEqual(['title', badge.id])
  })

  it('never leaves the table with nothing to show', () => {
    expect(visibleColumns([], allColumns()).map((c) => c.id)).toEqual(['title'])
    expect(toggleColumn(['title'], 'title', allColumns())).toEqual(['title'])
  })

  it('lets a flexible column share the leftover space', () => {
    const t = gridTemplate(visibleColumns(['title', 'year'], allColumns()), {})
    expect(t).toBe('minmax(160px, 1fr) 52px')
  })

  it('prefers a user width over the default', () => {
    const t = gridTemplate(visibleColumns(['title'], allColumns()), { title: 300 })
    expect(t).toBe('300px')
  })

  it('adds a column back in catalogue order, not at the end', () => {
    const next = toggleColumn(['title', 'modified'], 'year', allColumns())
    expect(next).toEqual(['title', 'year', 'modified'])
  })

  it('removes a column that was shown', () => {
    expect(toggleColumn(['title', 'year'], 'year', allColumns())).toEqual(['title'])
  })

  it('moves a column one place and stops at the ends', () => {
    expect(moveColumn(['a', 'b', 'c'], 'b', -1)).toEqual(['b', 'a', 'c'])
    expect(moveColumn(['a', 'b', 'c'], 'c', 1)).toEqual(['a', 'b', 'c'])
    expect(moveColumn(['a', 'b', 'c'], 'z', 1)).toEqual(['a', 'b', 'c'])
  })
})
