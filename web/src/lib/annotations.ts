/** Annotations on a PDF.
 *
 *  An annotation is an ordinary child item of the attachment, which is why
 *  there is no annotation API: highlights are searchable, exportable and
 *  syncable through the machinery items already have. This module owns only the
 *  geometry — turning a browser text selection into coordinates that survive
 *  zooming, and back again.
 */
import type { Item } from '../api/types'

/** Highlight colours, from the same palette collections use. */
export const HIGHLIGHT_COLOURS = ['amber', 'green', 'blue', 'violet', 'red'] as const

export type HighlightColour = (typeof HIGHLIGHT_COLOURS)[number]

/**
 * A rectangle in PDF page space, as fractions of the page.
 *
 * Fractions rather than pixels because the same annotation must land correctly
 * at any zoom, on any screen, and after the viewer is resized — storing device
 * pixels would tie a highlight to the window it was made in.
 */
export interface Rect {
  x: number
  y: number
  w: number
  h: number
}

export interface Position {
  page: number
  rects: Rect[]
}

export interface Annotation {
  key: string
  page: number
  rects: Rect[]
  text: string
  comment: string
  colour: HighlightColour
}

/** Parse the stored JSON, tolerating anything malformed. */
export function parsePosition(raw: unknown): Position | null {
  if (typeof raw !== 'string' || !raw) return null
  try {
    const value = JSON.parse(raw) as Position
    if (!Array.isArray(value?.rects) || !value.rects.length) return null
    return { page: Number(value.page) || 1, rects: value.rects }
  } catch {
    return null
  }
}

/** Read an annotation item into something the viewer can draw. */
export function toAnnotation(item: Item): Annotation | null {
  const position = parsePosition(item.annotationPosition)
  if (!position) return null
  const colour = String(item.annotationColor ?? 'amber')
  return {
    key: item.key,
    page: position.page,
    rects: position.rects,
    text: String(item.annotationText ?? ''),
    comment: String(item.annotationComment ?? ''),
    colour: (HIGHLIGHT_COLOURS as readonly string[]).includes(colour)
      ? (colour as HighlightColour)
      : 'amber',
  }
}

/** The fields to store, given a selection. */
export function toDraft(
  attachmentKey: string,
  position: Position,
  text: string,
  colour: HighlightColour,
) {
  return {
    itemType: 'annotation',
    parentKey: attachmentKey,
    annotationType: 'highlight',
    annotationText: text,
    annotationColor: colour,
    annotationPage: String(position.page),
    annotationPosition: JSON.stringify(position),
  }
}

/**
 * Turn a selection into page-relative rectangles.
 *
 * Selections spanning several lines produce several rectangles; merging them
 * into a bounding box would highlight the whitespace either side of a
 * paragraph, which looks like a mistake.
 */
export function rectsFromSelection(selection: Selection, page: DOMRect): Rect[] {
  const out: Rect[] = []
  for (let i = 0; i < selection.rangeCount; i += 1) {
    const range = selection.getRangeAt(i)
    for (const box of Array.from(range.getClientRects())) {
      // Zero-area rectangles come from collapsed ranges at line ends.
      if (box.width < 1 || box.height < 1) continue
      out.push({
        x: (box.left - page.left) / page.width,
        y: (box.top - page.top) / page.height,
        w: box.width / page.width,
        h: box.height / page.height,
      })
    }
  }
  return merge(out)
}

/**
 * Drop rectangles wholly inside another.
 *
 * A selection crossing element boundaries reports the same visual line more
 * than once, and drawing it twice makes the highlight visibly darker there.
 */
function merge(rects: Rect[]): Rect[] {
  return rects.filter(
    (r, i) =>
      !rects.some(
        (other, j) =>
          j !== i &&
          other.x <= r.x + 0.001 &&
          other.y <= r.y + 0.001 &&
          other.x + other.w >= r.x + r.w - 0.001 &&
          other.y + other.h >= r.y + r.h - 0.001 &&
          // Keep the first of two identical rectangles rather than neither.
          (other.w * other.h > r.w * r.h || j < i),
      ),
  )
}

/** Reading order: down the page, then across. */
export function inReadingOrder(annotations: Annotation[]): Annotation[] {
  return [...annotations].sort(
    (a, b) => a.page - b.page || (a.rects[0]?.y ?? 0) - (b.rects[0]?.y ?? 0),
  )
}
