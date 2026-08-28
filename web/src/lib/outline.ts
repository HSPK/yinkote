/**
 * A PDF's own table of contents.
 *
 * Most published papers have none; conference proceedings, theses and books
 * usually do, and in a two-hundred-page thesis it is the difference between
 * reading and hunting. So the panel has to be genuinely absent rather than
 * empty when there is nothing — an "Outline" tab that is always there and
 * usually blank teaches people to ignore it.
 *
 * The work here is turning a destination into a page number. A PDF bookmark
 * points at an object reference, not a page, and resolving one is an async call
 * into pdf.js. Doing that lazily per click would make the first click on every
 * heading slow and the rest instant, for no reason; doing it eagerly for a
 * thousand headings would block the panel. So it is done once, in parallel,
 * when the outline is read.
 */

import type { PDFDocumentProxy } from 'pdfjs-dist'

export interface OutlineNode {
  title: string
  /** 1-based, as printed on the page. Null when the destination cannot be
   *  resolved — a broken bookmark is common in generated PDFs, and a heading
   *  that does nothing is better than one that jumps somewhere wrong. */
  page: number | null
  depth: number
  children: OutlineNode[]
}

/** What pdf.js hands back, narrowed to the parts used here. */
interface RawNode {
  title: string
  dest: string | unknown[] | null
  items?: RawNode[]
}

/**
 * Resolve one bookmark's destination to a 1-based page number.
 *
 * A destination is either a named one (a string to look up) or an explicit
 * array whose first element is a page reference. Both end at `getPageIndex`.
 */
export async function pageOf(doc: PDFDocumentProxy, dest: RawNode['dest']): Promise<number | null> {
  try {
    const resolved = typeof dest === 'string' ? await doc.getDestination(dest) : dest
    const ref = Array.isArray(resolved) ? resolved[0] : null
    if (!ref || typeof ref !== 'object') return null
    const index = await doc.getPageIndex(ref as never)
    return index + 1
  } catch {
    // A bookmark pointing at nothing is a defect in the file, not in us.
    return null
  }
}

/**
 * The document's outline with every destination already resolved.
 *
 * Returns an empty array when the PDF has no outline, which the caller shows as
 * an absent panel rather than an empty one.
 */
export async function loadOutline(doc: PDFDocumentProxy): Promise<OutlineNode[]> {
  const raw = (await doc.getOutline().catch(() => null)) as RawNode[] | null
  if (!raw?.length) return []

  // Every destination at once. Resolving them one at a time down a deep tree
  // is a round trip per heading, in series.
  const pending: Promise<void>[] = []
  const convert = (nodes: RawNode[], depth: number): OutlineNode[] =>
    nodes.map((node) => {
      const out: OutlineNode = {
        title: (node.title || '').trim() || '—',
        page: null,
        depth,
        children: convert(node.items ?? [], depth + 1),
      }
      pending.push(
        pageOf(doc, node.dest).then((page) => {
          out.page = page
        }),
      )
      return out
    })

  const tree = convert(raw, 0)
  await Promise.all(pending)
  return tree
}

/** The tree as one list, which is what a panel actually renders. */
export function flatten(nodes: OutlineNode[]): OutlineNode[] {
  return nodes.flatMap((node) => [node, ...flatten(node.children)])
}
