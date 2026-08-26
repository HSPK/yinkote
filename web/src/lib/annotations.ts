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

/**
 * Which coordinate system a position is written in.
 *
 * `fraction` is this project's own: fractions of the page from the top-left,
 * which survive zooming, resizing and a different screen.
 *
 * `pdf` is what an imported Zotero highlight carries: points from the
 * bottom-left, exactly as the other program wrote them. It is kept unconverted
 * until the page is open, because converting needs the page's size in points
 * and that is inside the PDF. Guessing it would misplace every highlight in
 * every paper that is not the size guessed — silently, and only for people who
 * imported a library.
 */
export type Space = 'fraction' | 'pdf'

export interface Position {
  page: number
  rects: Rect[]
  space: Space
}

export interface Annotation {
  key: string
  page: number
  rects: Rect[]
  space: Space
  text: string
  comment: string
  colour: HighlightColour
}

/**
 * Parse the stored JSON, tolerating anything malformed.
 *
 * Two shapes are accepted, told apart by their rectangles rather than by a
 * flag: ours are objects, Zotero's are four-number arrays. A shape that cannot
 * be recognised is no position at all, and an annotation without one is not
 * drawn rather than drawn in the corner.
 */
export function parsePosition(raw: unknown): Position | null {
  if (typeof raw !== 'string' || !raw) return null
  try {
    const value = JSON.parse(raw) as {
      page?: number
      pageIndex?: number
      rects?: unknown[]
    }
    const rects = value?.rects
    if (!Array.isArray(rects) || !rects.length) return null

    if (Array.isArray(rects[0])) {
      const quads = (rects as number[][]).filter((r) => r.length === 4 && r.every(isFinite))
      if (!quads.length) return null
      return {
        // Zotero counts pages from zero; every page number a reader sees here
        // counts from one.
        page: Number(value.pageIndex ?? 0) + 1,
        rects: quads.map(([x1 = 0, y1 = 0, x2 = 0, y2 = 0]) => ({
          x: Math.min(x1, x2),
          y: Math.min(y1, y2),
          w: Math.abs(x2 - x1),
          h: Math.abs(y2 - y1),
        })),
        space: 'pdf',
      }
    }

    return { page: Number(value.page) || 1, rects: rects as Rect[], space: 'fraction' }
  } catch {
    return null
  }
}

/**
 * The rectangles to draw, as fractions of the page.
 *
 * PDF space has its origin at the bottom-left and grows upwards, so the top of
 * a rectangle is measured down from the top of the page — getting this backwards
 * puts a highlight the same distance from the wrong edge, which looks plausible
 * on a centred paragraph and wrong everywhere else.
 */
export function drawableRects(annotation: Annotation, page: { width: number; height: number }) {
  if (annotation.space === 'fraction') return annotation.rects
  if (!page.width || !page.height) return []

  return annotation.rects.map((r) => ({
    x: r.x / page.width,
    y: (page.height - r.y - r.h) / page.height,
    w: r.w / page.width,
    h: r.h / page.height,
  }))
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
    space: position.space,
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
  position: Omit<Position, 'space'>,
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
  // Down the page, whichever way the page counts. PDF space measures upwards,
  // so sorting its numbers ascending would list a paper backwards.
  const down = (a: Annotation) => {
    const y = a.rects[0]?.y ?? 0
    return a.space === 'pdf' ? -y : y
  }
  return [...annotations].sort((a, b) => a.page - b.page || down(a) - down(b))
}
