import { rectsFromRanges, type Rect } from './annotations'

export interface PageSelection {
  page: number
  rects: Rect[]
  text: string
}

/** Read text-node ranges only: selecting a layer element can report the entire
 *  page rectangle as well as its text, covering whitespace and other columns. */
export function pdfSelection(root: HTMLElement, selection: Selection, visibleOnly = false): PageSelection[] {
  if (!selection.rangeCount || selection.isCollapsed) return []
  const textLayer = (node: Node | null) =>
    (node instanceof Element ? node : node?.parentElement)?.closest('.pdf-text')
  const start = textLayer(selection.anchorNode)
  const end = textLayer(selection.focusNode)
  if (!start || !end || !root.contains(start) || !root.contains(end)) return []

  const result: PageSelection[] = []
  const viewport = visibleOnly ? root.getBoundingClientRect() : null
  for (const page of root.querySelectorAll<HTMLElement>('.pdf-page')) {
    const layer = page.querySelector('.pdf-text')
    if (!layer) continue
    const box = page.getBoundingClientRect()
    if (viewport && (box.bottom <= viewport.top || box.top >= viewport.bottom ||
      box.right <= viewport.left || box.left >= viewport.right)) continue
    const ranges: Range[] = []
    for (let i = 0; i < selection.rangeCount; i += 1) {
      const selected = selection.getRangeAt(i)
      if (!selected.intersectsNode(layer)) continue
      const walker = document.createTreeWalker(layer, NodeFilter.SHOW_TEXT)
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        if (!selected.intersectsNode(node)) continue
        const range = document.createRange()
        range.selectNodeContents(node)
        if (node === selected.startContainer) range.setStart(node, selected.startOffset)
        if (node === selected.endContainer) range.setEnd(node, selected.endOffset)
        if (!range.collapsed) ranges.push(range)
      }
    }
    const rects = rectsFromRanges(ranges, box)
    // Search highlighting splits a word into nested text nodes. Node boundaries
    // are not whitespace; keep exactly the text the reader selected.
    const text = ranges.map((range) => range.toString()).join('').trim()
    if (rects.length && text) result.push({ page: Number(page.dataset.page), rects, text })
  }
  return result
}
