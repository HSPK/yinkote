import { describe, expect, it } from 'vitest'
import { toolbarPosition, type ScreenRect } from './selection-toolbar'

const size = { width: 220, height: 36 }
const viewport = { width: 1000, height: 700 }

function overlaps(position: { x: number; y: number }, rect: ScreenRect): boolean {
  return position.x < rect.left + rect.width && position.x + size.width > rect.left &&
    position.y < rect.top + rect.height && position.y + size.height > rect.top
}

describe('placing selection tools', () => {
  it('sits above the whole multiline selection, not on its penultimate line', () => {
    const selection = [
      { left: 300, top: 200, width: 350, height: 20 },
      { left: 300, top: 230, width: 350, height: 20 },
      { left: 300, top: 260, width: 250, height: 20 },
    ]
    const position = toolbarPosition({ x: 425, y: 252 }, selection, size, viewport)
    expect(position.y + size.height).toBeLessThan(200)
    expect(selection.some((rect) => overlaps(position, rect))).toBe(false)
  })

  it('moves below a selection at the top instead of clamping back onto it', () => {
    const selection = [{ left: 10, top: 8, width: 500, height: 30 }]
    const position = toolbarPosition({ x: 20, y: 0 }, selection, size, viewport)
    expect(position.y).toBeGreaterThan(38)
    expect(position.x).toBeGreaterThanOrEqual(8)
    expect(overlaps(position, selection[0]!)).toBe(false)
  })

  it('docks outside a tall selection and stays inside the window', () => {
    const selection = [{ left: 100, top: 90, width: 850, height: 590 }]
    const position = toolbarPosition({ x: 990, y: 670 }, selection, size, viewport)
    expect(position.x + size.width).toBeLessThanOrEqual(992)
    expect(position.y + size.height).toBeLessThan(90)
    expect(overlaps(position, selection[0]!)).toBe(false)
  })
})
