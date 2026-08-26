/** Drag-to-resize splitters and persisted pane sizes.
 *
 *  A workbench should adapt to what the user is doing — a wide detail pane when
 *  editing metadata, a wide table when scanning results. Sizes are stored
 *  server-side alongside the other UI preferences so a layout follows the user
 *  between browsers.
 */
import { useCallback, useEffect, useRef, useState } from 'react'

export interface SplitterProps {
  /** Current size of the pane being resized, in pixels. */
  size: number
  onResize: (size: number) => void
  /** Committed once, when the drag ends — avoids a write per mouse move. */
  onCommit?: (size: number) => void
  min: number
  max: number
  /** `left` grows the pane to the splitter's left; `right` the one to its right. */
  grows: 'left' | 'right'
}

export function Splitter({ size, onResize, onCommit, min, max, grows }: SplitterProps) {
  const [dragging, setDragging] = useState(false)
  const start = useRef({ x: 0, size: 0 })
  const latest = useRef(size)

  latest.current = size

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault()
    start.current = { x: e.clientX, size }
    setDragging(true)
    e.currentTarget.setPointerCapture(e.pointerId)
  }

  const clamp = useCallback((value: number) => Math.max(min, Math.min(max, value)), [min, max])

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging) return
    const delta = e.clientX - start.current.x
    onResize(clamp(start.current.size + (grows === 'left' ? delta : -delta)))
  }

  const end = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging) return
    setDragging(false)
    e.currentTarget.releasePointerCapture(e.pointerId)
    onCommit?.(latest.current)
  }

  // A drag must not select text or leave the cursor looking wrong when the
  // pointer strays outside the handle.
  useEffect(() => {
    if (!dragging) return
    const previous = document.body.style.cursor
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
    return () => {
      document.body.style.cursor = previous
      document.body.style.userSelect = ''
    }
  }, [dragging])

  return (
    <div
      className="splitter"
      role="separator"
      aria-orientation="vertical"
      aria-valuenow={size}
      tabIndex={0}
      data-dragging={dragging || undefined}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={end}
      onPointerCancel={end}
      onDoubleClick={() => {
        // Double-click resets to the midpoint of the allowed range, which is a
        // cheap way back from an accidental drag to the edge.
        const reset = clamp(Math.round((min + max) / 2))
        onResize(reset)
        onCommit?.(reset)
      }}
      onKeyDown={(e) => {
        const step = e.shiftKey ? 40 : 8
        if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
          e.preventDefault()
          const direction = e.key === 'ArrowLeft' ? -1 : 1
          const next = clamp(size + direction * step * (grows === 'left' ? 1 : -1))
          onResize(next)
          onCommit?.(next)
        }
      }}
    />
  )
}
