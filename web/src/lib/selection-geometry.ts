import type { ScreenRect } from './selection-toolbar'

function canJoin(row: ScreenRect, box: ScreenRect) {
  const contained = box.left >= row.left && box.top >= row.top &&
    box.left + box.width <= row.left + row.width &&
    box.top + box.height <= row.top + row.height
  const overlap = Math.min(row.top + row.height, box.top + box.height) - Math.max(row.top, box.top)
  const middle = Math.abs(row.top + row.height / 2 - box.top - box.height / 2)
  const gap = Math.max(row.left - box.left - box.width, box.left - row.left - row.width, 0)
  return contained || (
    overlap >= Math.min(row.height, box.height) * 0.65 &&
    middle <= Math.max(row.height, box.height) * 0.45 &&
    gap <= Math.min(row.height, box.height) * 0.6
  )
}

function include(row: ScreenRect, box: ScreenRect) {
  const right = Math.max(row.left + row.width, box.left + box.width)
  const bottom = Math.max(row.top + row.height, box.top + box.height)
  row.left = Math.min(row.left, box.left)
  row.top = Math.min(row.top, box.top)
  row.width = right - row.left
  row.height = bottom - row.top
}

/** Join touching words on the same visual line, never the bounding rectangle
 *  of a paragraph. A gap wider than normal word spacing remains a column gap. */
export function continuousRects(input: readonly ScreenRect[]): ScreenRect[] {
  const rows: ScreenRect[] = []
  const boxes = input.filter((box) =>
    [box.left, box.top, box.width, box.height].every(Number.isFinite) &&
    box.width > 0 && box.height > 0,
  ).sort((a, b) => a.top - b.top || a.left - b.left)
  for (const box of boxes) {
    const matching = rows.find((row) => canJoin(row, box))
    if (!matching) {
      rows.push({ ...box })
      continue
    }
    include(matching, box)
    // A lower-baseline fragment can connect two groups already encountered.
    for (let i = rows.length - 1; i >= 0; i--) {
      const other = rows[i]!
      if (other !== matching && canJoin(matching, other)) {
        include(matching, other)
        rows.splice(i, 1)
      }
    }
  }
  return rows
}
