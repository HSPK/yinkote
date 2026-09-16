export interface ScreenRect {
  left: number
  top: number
  width: number
  height: number
}

/** Prefer above the entire selection, not above its last line. Clamping that
 *  last-line position at the screen edge can put a toolbar back over the words. */
export function toolbarPosition(
  at: { x: number; y: number },
  selected: readonly ScreenRect[],
  size: { width: number; height: number },
  viewport: { width: number; height: number },
): { x: number; y: number } {
  const margin = 8
  const gap = 10
  const width = Math.min(size.width, viewport.width - margin * 2)
  const height = size.height
  const clampX = (x: number) => Math.max(margin, Math.min(viewport.width - width - margin, x))
  const visible = selected.filter((r) => r.top + r.height > 0 && r.top < viewport.height)
  const top = visible.length ? Math.min(...visible.map((r) => r.top)) : at.y
  const bottom = visible.length ? Math.max(...visible.map((r) => r.top + r.height)) : at.y
  const left = visible.length ? Math.min(...visible.map((r) => r.left)) : at.x
  const right = visible.length ? Math.max(...visible.map((r) => r.left + r.width)) : at.x
  const x = clampX(at.x - width / 2)
  const candidates = [
    { x, y: top - height - gap },
    { x, y: bottom + gap },
    { x: right + gap, y: Math.max(margin, top) },
    { x: left - width - gap, y: Math.max(margin, top) },
    { x, y: margin },
    { x, y: viewport.height - height - margin },
  ]
  const fits = (p: { x: number; y: number }) =>
    p.x >= margin && p.y >= margin &&
    p.x + width <= viewport.width - margin && p.y + height <= viewport.height - margin &&
    visible.every((r) =>
      p.x + width + gap <= r.left || p.x >= r.left + r.width + gap ||
      p.y + height + gap <= r.top || p.y >= r.top + r.height + gap,
    )
  // A selection in the PDF always leaves the workbench toolbar outside its
  // scroll viewport. The final candidate docks there if neither side fits.
  return candidates.find(fits) ?? { x, y: margin }
}
