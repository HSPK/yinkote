import { describe, expect, it } from 'vitest'

import { canvasOutput, MAX_CANVAS_DIMENSION, MAX_CANVAS_PIXELS } from './pdfRendering'

describe('canvas output limits', () => {
  it.each([
    [600.25, 800.75, 2],
    [72000, 96000, 4],
    [1, 1000000, 3],
    [1000000, 1, 3],
    [1e200, 1e200, 4],
  ])('bounds %s × %s at device ratio %s', (width, height, ratio) => {
    const output = canvasOutput(width, height, ratio)
    expect(output.width).toBeGreaterThanOrEqual(1)
    expect(output.height).toBeGreaterThanOrEqual(1)
    expect(output.width).toBeLessThanOrEqual(MAX_CANVAS_DIMENSION)
    expect(output.height).toBeLessThanOrEqual(MAX_CANVAS_DIMENSION)
    expect(output.width * output.height).toBeLessThanOrEqual(MAX_CANVAS_PIXELS)
    expect(output.scaleX * width).toBeCloseTo(output.width)
    expect(output.scaleY * height).toBeCloseTo(output.height)
  })

  it.each([0, -1, NaN, Infinity])('falls back for an invalid device ratio %s', (ratio) => {
    expect(canvasOutput(600, 800, ratio)).toEqual(canvasOutput(600, 800, 1))
  })

  it.each([0, -1, NaN, Infinity])('rejects invalid page dimensions %s', (dimension) => {
    expect(() => canvasOutput(dimension, 800, 1)).toThrow('Invalid PDF page dimensions')
    expect(() => canvasOutput(600, dimension, 1)).toThrow('Invalid PDF page dimensions')
  })
})
