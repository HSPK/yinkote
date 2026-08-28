/** A large secondary surface for things that are not the main workflow.
 *
 *  Plugins, status and settings are all "step aside, do a thing, come back" —
 *  they do not deserve to displace the library. A modal keeps the workbench
 *  visible behind them and costs one Escape to leave.
 */
import { useRef, type ReactNode } from 'react'

import { Icon } from './Icon'
import { useDismissable } from './useDismissable'

export interface ModalProps {
  title: string
  onClose: () => void
  /** `wide` suits inventories and dashboards; `narrow` suits forms. */
  width?: 'narrow' | 'wide'
  /** Set false when the content scrolls its own panes; the body then fills the
   *  modal instead of scrolling as one document, which is what lets a side rail
   *  stay put while the rest moves. */
  scroll?: boolean
  children: ReactNode
}

export function Modal({ title, onClose, width = 'wide', scroll = true, children }: ModalProps) {
  const panel = useRef<HTMLDivElement>(null)
  useDismissable(panel, true, onClose)

  return (
    <div className="overlay">
      <div className="modal" ref={panel} data-width={width} data-fill={!scroll || undefined}>
        <header className="modal-head">
          <span>{title}</span>
          {/* i18n-exempt: the legend printed on the key itself, which keyboards do not translate */}
          <button className="modal-close" onClick={onClose} title="Esc">
            <Icon.Close size={13} />
          </button>
        </header>
        <div className="modal-body" data-scroll={scroll}>
          {children}
        </div>
      </div>
    </div>
  )
}
