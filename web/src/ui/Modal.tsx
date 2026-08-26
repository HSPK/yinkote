/** A large secondary surface for things that are not the main workflow.
 *
 *  Plugins, status and settings are all "step aside, do a thing, come back" —
 *  they do not deserve to displace the library. A modal keeps the workbench
 *  visible behind them and costs one Escape to leave.
 */
import { useEffect, type ReactNode } from 'react'

import { Icon } from './Icon'

export interface ModalProps {
  title: string
  onClose: () => void
  /** `wide` suits inventories and dashboards; `narrow` suits forms. */
  width?: 'narrow' | 'wide'
  children: ReactNode
}

export function Modal({ title, onClose, width = 'wide', children }: ModalProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation()
        onClose()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="modal" data-width={width} onMouseDown={(e) => e.stopPropagation()}>
        <header className="modal-head">
          <span>{title}</span>
          <button className="modal-close" onClick={onClose} title="Esc">
            <Icon.Close size={13} />
          </button>
        </header>
        <div className="modal-body">{children}</div>
      </div>
    </div>
  )
}
