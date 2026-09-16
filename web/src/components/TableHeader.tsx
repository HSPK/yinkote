import { Fragment, useCallback, useEffect, useLayoutEffect, useMemo, useRef, type ReactNode } from 'react'

import { useT } from '../i18n'
import {
  CATALOGUE, DEFAULT_COLUMNS, columnWidthKey, gridTemplate, reorderColumn,
  toggleColumn, totalColumnWidth, visibleColumns, type ColumnDef, type TableId,
} from '../lib/columns'
import { endDrag } from '../lib/dnd'
import { useStore } from '../state/store'
import { useOverlays, type MenuItem } from '../ui/overlays'
import './table-columns.css'

export const COLUMN_DRAG_TYPE = 'application/x-yinkote-column'

interface ColumnOptions {
  available?: ColumnDef[]
  label?: (column: ColumnDef) => string
  actionsWidth?: number
}

/** One catalogue and live order drive the header, cells and context menu. */
export function useTableColumns(table: TableId, options: ColumnOptions = {}) {
  const t = useT()
  const available = options.available ?? CATALOGUE[table]
  const label = options.label ?? ((column: ColumnDef) => t(column.labelKey))
  const order = useStore((s) => s.columnOrders[table] ?? DEFAULT_COLUMNS[table])
  const savedWidths = useStore((s) => s.columnWidths)
  const columns = useMemo(() => visibleColumns(order, available), [order, available])
  const widths = useMemo(() => Object.fromEntries(available.map((c) =>
    [c.id, savedWidths[columnWidthKey(table, c.id)] ?? c.width],
  )), [table, available, savedWidths])
  const actionsWidth = options.actionsWidth ?? 0
  const grid = gridTemplate(columns, widths) + (actionsWidth ? ` ${actionsWidth}px` : '')
  const width = totalColumnWidth(columns, widths) + actionsWidth
  const latest = useRef({ available, label, t })
  latest.current = { available, label, t }
  const contextColumn = useRef<string>()

  const liveOrder = useCallback(() => {
    const state = useStore.getState()
    const order = state.columnOrders[table] ?? DEFAULT_COLUMNS[table]
    // Keep unavailable plugin preferences, but include the rendered fallback
    // when every stored column is unavailable.
    return [...new Set([...order, ...visibleColumns(order, latest.current.available).map((c) => c.id)])]
  }, [table])

  const move = useCallback((id: string, delta: number) => {
    const order = liveOrder()
    const shown = visibleColumns(order, latest.current.available).map((c) => c.id)
    const target = shown[Math.max(0, Math.min(shown.length - 1, shown.indexOf(id) + delta))]
    if (target) useStore.getState().setColumnOrder(table, reorderColumn(order, id, target))
  }, [table, liveOrder])

  const menu = useCallback((): MenuItem[] => {
    const { available, label, t } = latest.current
    const order = liveOrder()
    const shown = visibleColumns(order, available).map((c) => c.id)
    const toggle = (id: string) => {
      const state = useStore.getState()
      state.setColumnOrder(table, toggleColumn(liveOrder(), id, latest.current.available))
    }
    const entries: MenuItem[] = [{
      id: 'columns',
      label: t('table.columns'),
      items: [
        ...available.map((column) => ({
          id: column.id,
          label: label(column),
          checked: shown.includes(column.id),
          disabled: shown.length === 1 && shown[0] === column.id,
          onSelect: () => toggle(column.id),
        })),
        {},
        { id: 'reset', label: t('table.resetColumns'), onSelect: () => useStore.getState().resetColumns(table) },
      ],
    }]
    const id = contextColumn.current
    if (id && shown.includes(id)) entries.push(
      {},
      { label: t('table.moveLeft'), disabled: shown[0] === id, onSelect: () => move(id, -1) },
      { label: t('table.moveRight'), disabled: shown[shown.length - 1] === id, onSelect: () => move(id, 1) },
      { label: t('table.hideColumn'), disabled: shown.length === 1, onSelect: () => toggle(id) },
    )
    return entries
  }, [table, liveOrder, move])

  useLayoutEffect(() => {
    useOverlays.getState().refreshMenu(menu)
  })
  useEffect(() => () => {
    const overlays = useOverlays.getState()
    if (overlays.menu?.source === menu) overlays.closeMenu()
  }, [menu])

  const openMenu = (x: number, y: number, column?: string) => {
    contextColumn.current = column
    useOverlays.getState().openMenu(x, y, menu)
  }

  return { table, columns, grid, width, label, actionsWidth, openMenu, liveOrder, move }
}

type TableColumns = ReturnType<typeof useTableColumns>

export function TableCells({ layout, cells }: { layout: TableColumns; cells: Record<string, ReactNode> }) {
  return layout.columns.map((column) => <Fragment key={column.id}>{cells[column.id]}</Fragment>)
}

interface TableHeaderProps {
  layout: TableColumns
  className?: string
  sort?: string
  direction?: 'asc' | 'desc'
  onSort?: (sort: string) => void
  sortDisabled?: boolean
  sortHint?: string
  content?: (column: ColumnDef) => ReactNode
  resizable?: boolean
}

export function TableHeader({
  layout, className = '', sort, direction = 'asc', onSort,
  sortDisabled, sortHint, content, resizable = true,
}: TableHeaderProps) {
  const { table, columns, grid, width, label, actionsWidth, openMenu, liveOrder, move } = layout
  const t = useT()
  const suppressClick = useRef(0)
  const resizeCleanup = useRef<() => void>()
  useEffect(() => () => resizeCleanup.current?.(), [])

  const startResize = (column: ColumnDef, event: React.PointerEvent) => {
    event.preventDefault()
    event.stopPropagation()
    resizeCleanup.current?.()
    const from = event.clientX
    const base = event.currentTarget.parentElement?.getBoundingClientRect().width ?? column.min
    const key = columnWidthKey(table, column.id)
    let last = base
    const previousCursor = document.body.style.cursor
    const move = (event: PointerEvent) => {
      last = Math.max(column.min, Math.min(640, base + event.clientX - from))
      useStore.getState().setColumnWidth(key, last)
    }
    const cleanup = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      window.removeEventListener('pointercancel', up)
      document.body.style.cursor = previousCursor
      resizeCleanup.current = undefined
    }
    const up = () => {
      cleanup()
      suppressClick.current = Date.now() + 250
      useStore.getState().setColumnWidth(key, last, true)
    }
    resizeCleanup.current = cleanup
    document.body.style.cursor = 'col-resize'
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
    window.addEventListener('pointercancel', up)
  }

  return (
    <div
      className={`table-head column-header ${className}`.trim()}
      role="row"
      data-table={table}
      style={{ gridTemplateColumns: grid, minWidth: width }}
      onContextMenu={(event) => {
        event.preventDefault()
        event.stopPropagation()
        const cell = (event.target as HTMLElement).closest<HTMLElement>('[data-column]')
        openMenu(event.clientX, event.clientY, cell?.dataset.column)
      }}
    >
      {columns.map((column) => (
        <div
          key={column.id}
          className="head-cell"
          data-column={column.id}
          role="columnheader"
          aria-label={label(column)}
          aria-sort={!sortDisabled && sort === column.sort ? direction === 'asc' ? 'ascending' : 'descending' : undefined}
          tabIndex={0}
          title={sortDisabled ? sortHint : t('table.columnsHint')}
          draggable
          onKeyDown={(event) => {
            if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
              event.preventDefault()
              const rect = event.currentTarget.getBoundingClientRect()
              openMenu(rect.left, rect.bottom, column.id)
            } else if (event.altKey && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')) {
              event.preventDefault()
              move(column.id, event.key === 'ArrowLeft' ? -1 : 1)
            }
          }}
          onDragStart={(event) => {
            if (resizeCleanup.current) {
              event.preventDefault()
              return
            }
            event.stopPropagation()
            endDrag()
            event.dataTransfer.effectAllowed = 'move'
            event.dataTransfer.setData(COLUMN_DRAG_TYPE, JSON.stringify({ table, id: column.id }))
            suppressClick.current = Infinity
          }}
          onDragOver={(event) => {
            if (!Array.from(event.dataTransfer.types).includes(COLUMN_DRAG_TYPE)) return
            event.preventDefault()
            event.stopPropagation()
            event.dataTransfer.dropEffect = 'move'
          }}
          onDrop={(event) => {
            const raw = event.dataTransfer.getData(COLUMN_DRAG_TYPE)
            if (!raw) return
            event.preventDefault()
            event.stopPropagation()
            suppressClick.current = Date.now() + 250
            try {
              const source = JSON.parse(raw) as { table?: string; id?: string }
              if (source.table !== table || typeof source.id !== 'string') return
              const order = liveOrder()
              const next = reorderColumn(order, source.id, column.id)
              if (next !== order) useStore.getState().setColumnOrder(table, next)
            } catch {
              // Ignore unrelated or malformed drags.
            }
          }}
          onDragEnd={() => { suppressClick.current = Date.now() + 250 }}
        >
          <button
            className={!sortDisabled && sort === column.sort ? 'sorted' : undefined}
            disabled={!column.sort || !onSort || sortDisabled}
            title={sortDisabled ? sortHint : label(column)}
            aria-label={label(column)}
            onClick={() => {
              if (Date.now() >= suppressClick.current && column.sort) onSort?.(column.sort)
            }}
          >
            <span className="head-label">{content?.(column) ?? label(column)}</span>
            {!sortDisabled && sort === column.sort && (
              <span className="sort-arrow">{direction === 'asc' ? '↑' : '↓'}</span>
            )}
          </button>
          {resizable && (
            <span
              className="col-grip"
              onPointerDown={(event) => startResize(column, event)}
              onDoubleClick={(event) => {
                event.stopPropagation()
                useStore.getState().setColumnWidth(columnWidthKey(table, column.id), column.width, true)
              }}
            />
          )}
        </div>
      ))}
      {actionsWidth > 0 && <div className="head-cell" aria-hidden="true" />}
    </div>
  )
}
