import { useEffect, type RefObject } from 'react'

/**
 * Close a popover on a click elsewhere or on Escape.
 *
 * Both gestures, together, are what makes something a popover rather than a
 * thing stuck to the screen — and having written them separately three times,
 * they had drifted: one stopped propagation so an outer handler would not also
 * fire, the others did not, so Escape in a picker inside a modal closed both.
 *
 * The keydown listener captures, so the innermost open surface answers first.
 */
export function useDismissable(
  root: RefObject<HTMLElement | null>,
  open: boolean,
  onClose: () => void,
): void {
  useEffect(() => {
    if (!open) return

    const onPointerDown = (e: MouseEvent) => {
      if (!root.current?.contains(e.target as Node)) onClose()
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.stopPropagation()
      onClose()
    }

    document.addEventListener('mousedown', onPointerDown)
    document.addEventListener('keydown', onKeyDown, true)
    return () => {
      document.removeEventListener('mousedown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown, true)
    }
  }, [open, onClose, root])
}
