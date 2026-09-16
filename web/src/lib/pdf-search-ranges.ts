import type { PdfSearchMatch } from './pdf-search'

/** Text-only ranges avoid the duplicate element rects of a cross-span Range. */
export function pdfSearchRanges(
  host: HTMLElement,
  matches: readonly PdfSearchMatch[],
): { matchId: string; range: Range }[] {
  const nodes: { node: Text; start: number; end: number }[] = []
  const walker = host.ownerDocument.createTreeWalker(host, NodeFilter.SHOW_TEXT)
  let length = 0
  while (walker.nextNode()) {
    const node = walker.currentNode as Text
    if (!node.length) continue
    nodes.push({ node, start: length, end: length + node.length })
    length += node.length
  }
  const ranges: { matchId: string; range: Range }[] = []
  for (const match of matches) {
    const start = match.start.textOffset
    const end = match.end.textOffset
    if (start < 0 || end > length || end <= start) continue
    for (const item of nodes) {
      if (item.start >= end) break
      if (item.end <= start) continue
      const range = host.ownerDocument.createRange()
      range.setStart(item.node, Math.max(start - item.start, 0))
      range.setEnd(item.node, Math.min(end - item.start, item.node.length))
      ranges.push({ matchId: match.id, range })
    }
  }
  return ranges
}
