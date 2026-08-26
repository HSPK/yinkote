import { useRef } from 'react'

import { useT } from '../i18n'
import { moveColumn, toggleColumn, type ColumnDef } from '../lib/columns'
import { useStore } from '../state/store'
import { Button, Icon, useDismissable } from '../ui'

export interface ColumnPickerProps {
  available: ColumnDef[]
  label: (column: ColumnDef) => string
  onClose: () => void
}

/**
 * Chooses and orders the table's columns.
 *
 * A real popover rather than a context menu, because this is a stateful
 * multi-select: a menu captures its items when it opens, so every click after
 * the first acted on a stale list — turning one column on quietly turned the
 * previous one off again. Here the ticks and the order come from the store on
 * every render, so what is shown is what is stored.
 */
export function ColumnPicker({ available, label, onClose }: ColumnPickerProps) {
  const t = useT()
  const order = useStore((s) => s.columnOrder)
  const setColumnOrder = useStore((s) => s.setColumnOrder)
  const resetColumns = useStore((s) => s.resetColumns)
  const root = useRef<HTMLDivElement>(null)

  useDismissable(root, true, onClose)


  // Shown columns first, in their display order, then the rest to pick from.
  const shown = order.filter((id) => available.some((c) => c.id === id))
  const hidden = available.filter((c) => !order.includes(c.id)).map((c) => c.id)
  const byId = new Map(available.map((c) => [c.id, c]))

  const row = (id: string, visible: boolean, index: number) => {
    const column = byId.get(id)
    if (!column) return null
    return (
      <div className="column-row" key={id}>
        <button
          className="column-toggle"
          data-checked={visible || undefined}
          // The live order is read here, not captured when the popover opened.
          onClick={() => setColumnOrder(toggleColumn(order, id, available))}
        >
          <span className="column-check">{visible ? '✓' : ''}</span>
          {label(column)}
        </button>
        {visible && (
          <span className="column-move">
            <button
              className="icon-btn"
              title={t('table.moveLeft')}
              disabled={index === 0}
              onClick={() => setColumnOrder(moveColumn(order, id, -1))}
            >
              <Icon.ChevronUp size={11} />
            </button>
            <button
              className="icon-btn"
              title={t('table.moveRight')}
              disabled={index === shown.length - 1}
              onClick={() => setColumnOrder(moveColumn(order, id, 1))}
            >
              <Icon.ChevronDown size={11} />
            </button>
          </span>
        )}
      </div>
    )
  }

  return (
    <div className="column-pop" ref={root}>
      <div className="column-head">{t('table.columnsHint')}</div>
      <div className="column-list">
        {shown.map((id, i) => row(id, true, i))}
        {hidden.length > 0 && <div className="column-sep" />}
        {hidden.map((id) => row(id, false, -1))}
      </div>
      <div className="column-foot">
        <Button tone="ghost" onClick={resetColumns}>
          {t('table.resetColumns')}
        </Button>
        <Button onClick={onClose}>{t('dialog.confirm')}</Button>
      </div>
    </div>
  )
}
