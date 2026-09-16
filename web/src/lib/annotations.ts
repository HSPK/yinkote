/** Annotations on a PDF.
 *
 *  An annotation is an ordinary child item of the attachment, which is why
 *  there is no annotation API: highlights are searchable, exportable and
 *  syncable through the machinery items already have. This module owns only the
 *  geometry — turning a browser text selection into coordinates that survive
 *  zooming, and back again.
 */
import type { Item } from '../api/types'
import { continuousRects } from './selection-geometry'
import type { ScreenRect } from './selection-toolbar'

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

/** What a mark on the page is.
 *
 *  The same names Zotero uses, so an imported library keeps its underlines and
 *  an exported one is still readable there. Anything unrecognised is treated as
 *  a highlight: an annotation drawn the wrong way is recoverable, one that is
 *  not drawn at all looks like data loss. */
export const MARKS = ['highlight', 'underline'] as const
export type Mark = (typeof MARKS)[number]

export interface Annotation {
  key: string
  version?: number
  kind: Mark
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
  const kind = String(item.annotationType ?? 'highlight')
  return {
    key: item.key,
    version: item.version,
    page: position.page,
    rects: position.rects,
    space: position.space,
    text: String(item.annotationText ?? ''),
    comment: String(item.annotationComment ?? ''),
    kind: (MARKS as readonly string[]).includes(kind) ? (kind as Mark) : 'highlight',
    colour: (HIGHLIGHT_COLOURS as readonly string[]).includes(colour)
      ? (colour as HighlightColour)
      : 'amber',
  }
}

/** Hit-test through the text layer, so marks stay clickable without blocking
 *  native text selection with a pointer-catching overlay. */
export function annotationAt(
  annotations: Annotation[],
  point: { x: number; y: number },
  page: { width: number; height: number },
): Annotation | undefined {
  return [...annotations].reverse().find((annotation) =>
    drawableRects(annotation, page).some((rect) =>
      point.x >= rect.x && point.x <= rect.x + rect.w &&
      point.y >= rect.y && point.y <= rect.y + rect.h,
    ),
  )
}

/** The fields to store, given a selection. */
export function toDraft(
  attachmentKey: string,
  position: Omit<Position, 'space'>,
  text: string,
  colour: HighlightColour,
  kind: Mark = 'highlight',
) {
  return {
    itemType: 'annotation',
    parentKey: attachmentKey,
    annotationType: kind,
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
  return rectsFromRanges(
    Array.from({ length: selection.rangeCount }, (_, i) => selection.getRangeAt(i)),
    page,
  )
}

export function rectsFromRanges(ranges: Range[], page: DOMRect): Rect[] {
  const out: ScreenRect[] = []
  if (page.width <= 0 || page.height <= 0) return []
  for (const range of ranges) {
    for (const box of Array.from(range.getClientRects())) {
      const left = Math.max(box.left, page.left)
      const top = Math.max(box.top, page.top)
      const right = Math.min(box.left + box.width, page.left + page.width)
      const bottom = Math.min(box.top + box.height, page.top + page.height)
      if (right - left < 1 || bottom - top < 1) continue
      out.push({
        left, top, width: right - left, height: bottom - top,
      })
    }
  }
  return continuousRects(out).map((box) => ({
    x: (box.left - page.left) / page.width,
    y: (box.top - page.top) / page.height,
    w: box.width / page.width,
    h: box.height / page.height,
  }))
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
