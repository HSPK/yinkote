import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { toolbarPosition, type ScreenRect } from '../lib/selection-toolbar'

const EMPTY_RECTS: ScreenRect[] = []

/** Shared opaque, selection-aware placement for new and existing marks. */
export function FloatingTools({ at, selection = EMPTY_RECTS, label, onDismiss, children, className = '' }: {
  at: { x: number; y: number }
  selection?: readonly ScreenRect[]
  label: string
  onDismiss: () => void
  children: ReactNode
  className?: string
}) {
  const root = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState(at)
  useLayoutEffect(() => {
    const place = () => {
      const box = root.current?.getBoundingClientRect()
      if (!box) return
      const next = toolbarPosition(at, selection, box, { width: window.innerWidth, height: window.innerHeight })
      setPosition((old) => old.x === next.x && old.y === next.y ? old : next)
    }
    place()
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(place)
    if (root.current) observer?.observe(root.current)
    return () => observer?.disconnect()
  }, [at.x, at.y, selection])

  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.preventDefault()
      onDismiss()
    }
    const outside = (event: MouseEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) onDismiss()
    }
    window.addEventListener('keydown', key)
    window.addEventListener('resize', onDismiss)
    document.addEventListener('mousedown', outside)
    return () => {
      window.removeEventListener('keydown', key)
      window.removeEventListener('resize', onDismiss)
      document.removeEventListener('mousedown', outside)
    }
  }, [onDismiss])

  return (
    <div ref={root} className={`selection-popup ${className}`.trim()} role="toolbar"
      aria-label={label} style={{ left: position.x, top: position.y }}
      onMouseDown={(event) => {
        event.stopPropagation()
        if (!(event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement)) {
          event.preventDefault()
        }
      }}>
      {children}
    </div>
  )
}
