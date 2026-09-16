import { describe, expect, it } from 'vitest'
import { continuousRects } from './selection-geometry'

describe('continuous selection geometry', () => {
  it('fills ordinary word spaces on one line', () => {
    expect(continuousRects([
      { left: 10, top: 20, width: 30, height: 14 },
      { left: 44, top: 20, width: 35, height: 14 },
      { left: 83, top: 20, width: 20, height: 14 },
    ])).toEqual([{ left: 10, top: 20, width: 93, height: 14 }])
  })

  it('keeps line breaks, column gutters and table-cell gaps separate', () => {
    const boxes = [
      { left: 10, top: 20, width: 60, height: 14 },
      { left: 150, top: 20, width: 60, height: 14 },
      { left: 10, top: 40, width: 60, height: 14 },
      { left: 85, top: 40, width: 60, height: 14 },
    ]
    expect(continuousRects(boxes)).toEqual(boxes)
  })

  it('removes duplicate glyph rectangles and joins mixed fonts on a baseline', () => {
    expect(continuousRects([
      { left: 10, top: 20, width: 30, height: 14 },
      { left: 10, top: 20, width: 30, height: 14 },
      { left: 44, top: 21, width: 35, height: 12 },
      { left: 82, top: 20, width: 20, height: 14 },
    ])).toEqual([{ left: 10, top: 20, width: 92, height: 14 }])
  })

  it('scales identically at different zoom levels and leaves input untouched', () => {
    const boxes = [
      { left: 10, top: 20, width: 30, height: 14 },
      { left: 44, top: 20, width: 35, height: 14 },
    ]
    const scale = (box: typeof boxes[number]) => ({
      left: box.left * 3, top: box.top * 3, width: box.width * 3, height: box.height * 3,
    })
    expect(continuousRects(boxes.map(scale))).toEqual(continuousRects(boxes).map(scale))
    expect(boxes[0]?.width).toBe(30)
  })
})
