import { describe, expect, it } from 'vitest'

import type { Item } from '../api/types'
import {
  inReadingOrder,
  parsePosition,
  rectsFromSelection,
  toAnnotation,
  toDraft,
  type Annotation,
} from './annotations'

const page = { left: 100, top: 50, width: 800, height: 1000 } as DOMRect

/** A stand-in for a browser Selection; jsdom does not lay anything out. */
function selection(boxes: Partial<DOMRect>[][]): Selection {
  return {
    rangeCount: boxes.length,
    getRangeAt: (i: number) => ({
      getClientRects: () => boxes[i]!.map((b) => ({ width: 0, height: 0, ...b }) as DOMRect),
    }),
  } as unknown as Selection
}

const item = (fields: Partial<Item>) => ({ key: 'A', ...fields }) as Item

describe('parsePosition', () => {
  it('reads a stored position', () => {
    const got = parsePosition('{"page":3,"rects":[{"x":0,"y":0,"w":1,"h":1}]}')
    expect(got).toEqual({ page: 3, rects: [{ x: 0, y: 0, w: 1, h: 1 }] })
  })

  it('refuses anything malformed rather than drawing nonsense', () => {
    expect(parsePosition('{not json')).toBeNull()
    expect(parsePosition('{"page":1,"rects":[]}')).toBeNull()
    expect(parsePosition(undefined)).toBeNull()
    expect(parsePosition(42)).toBeNull()
  })

  it('defaults a missing page to the first', () => {
    expect(parsePosition('{"rects":[{"x":0,"y":0,"w":1,"h":1}]}')?.page).toBe(1)
  })
})

describe('toAnnotation', () => {
  it('reads an annotation item', () => {
    const got = toAnnotation(
      item({
        annotationPosition: '{"page":2,"rects":[{"x":0,"y":0,"w":0.5,"h":0.1}]}',
        annotationText: 'attention',
        annotationColor: 'green',
      } as Partial<Item>),
    )
    expect(got).toMatchObject({ page: 2, text: 'attention', colour: 'green' })
  })

  it('falls back to a known colour, so a bad value cannot make it invisible', () => {
    const got = toAnnotation(
      item({
        annotationPosition: '{"page":1,"rects":[{"x":0,"y":0,"w":1,"h":1}]}',
        annotationColor: 'chartreuse',
      } as Partial<Item>),
    )
    expect(got?.colour).toBe('amber')
  })

  it('skips an annotation with no usable position', () => {
    expect(toAnnotation(item({ annotationText: 'x' } as Partial<Item>))).toBeNull()
  })
})

describe('rectsFromSelection', () => {
  it('stores fractions of the page, so zoom does not move a highlight', () => {
    const got = rectsFromSelection(
      selection([[{ left: 100, top: 50, width: 400, height: 100 }]]),
      page,
    )
    expect(got).toEqual([{ x: 0, y: 0, w: 0.5, h: 0.1 }])
  })

  it('keeps one rectangle per line rather than one bounding box', () => {
    // A bounding box would highlight the whitespace either side of a paragraph.
    const got = rectsFromSelection(
      selection([
        [
          { left: 500, top: 50, width: 300, height: 20 },
          { left: 100, top: 70, width: 700, height: 20 },
        ],
      ]),
      page,
    )
    expect(got).toHaveLength(2)
  })

  it('drops zero-area rectangles from collapsed ranges', () => {
    const got = rectsFromSelection(
      selection([[{ left: 100, top: 50, width: 0, height: 20 }]]),
      page,
    )
    expect(got).toEqual([])
  })

  it('drops a rectangle wholly inside another, which would darken that line', () => {
    const got = rectsFromSelection(
      selection([
        [
          { left: 100, top: 50, width: 400, height: 100 },
          { left: 200, top: 60, width: 100, height: 20 },
        ],
      ]),
      page,
    )
    expect(got).toHaveLength(1)
    expect(got[0]?.w).toBe(0.5)
  })

  it('keeps one of two identical rectangles, not neither', () => {
    const box = { left: 100, top: 50, width: 400, height: 100 }
    expect(rectsFromSelection(selection([[box, box]]), page)).toHaveLength(1)
  })
})

describe('toDraft', () => {
  it('produces an ordinary child item, which is why there is no annotation API', () => {
    const draft = toDraft('ATT1', { page: 2, rects: [{ x: 0, y: 0, w: 1, h: 1 }] }, 'text', 'blue')
    expect(draft).toMatchObject({
      itemType: 'annotation',
      parentKey: 'ATT1',
      annotationText: 'text',
      annotationColor: 'blue',
      annotationPage: '2',
    })
    expect(JSON.parse(draft.annotationPosition).page).toBe(2)
  })
})

describe('inReadingOrder', () => {
  it('sorts down the page, then across pages', () => {
    const at = (page: number, y: number) =>
      ({ key: `${page}-${y}`, page, rects: [{ x: 0, y, w: 1, h: 1 }] }) as Annotation
    const sorted = inReadingOrder([at(2, 0.1), at(1, 0.9), at(1, 0.2)])
    expect(sorted.map((a) => a.key)).toEqual(['1-0.2', '1-0.9', '2-0.1'])
  })
})
