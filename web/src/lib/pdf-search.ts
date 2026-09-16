export interface PdfTextRun {
  str: string
  hasEOL?: boolean
}

/** The subset of PDF.js used by search: no viewport, canvas, or text layer. */
export interface PdfSearchDocument {
  numPages: number
  getPage(pageNumber: number): Promise<{
    getTextContent(): Promise<{ items: (PdfTextRun | { type: string })[] }>
  }>
}

export interface PdfTextPosition {
  /** Text-item ordinal, excluding PDF.js marked-content metadata. */
  run: number
  /** UTF-16 offset within the original, unnormalised text item. */
  offset: number
  /** UTF-16 offset in concatenated item strings, without synthetic EOLs. */
  textOffset: number
}

export interface PdfSearchMatch {
  id: string
  pageNumber: number
  start: PdfTextPosition
  end: PdfTextPosition
}

export interface IndexedPdfText {
  text: string
  starts: PdfTextPosition[]
  ends: PdfTextPosition[]
}

function normalise(text: string): string {
  return text.normalize('NFKC').toLowerCase()
    .replace(/\u03c2/g, '\u03c3')
    .replace(/[\u00ad\u200b]/g, '')
}

export function pdfSearchQuery(query: string): string {
  return normalise(query).replace(/\s+/gu, ' ').trim()
}

/** Preserve source offsets even when one ligature expands into several letters. */
export function indexPdfText(items: (PdfTextRun | { type: string })[]): IndexedPdfText {
  const raw: string[] = []
  const sources: { start: PdfTextPosition; end: PdfTextPosition }[] = []
  let textOffset = 0
  let run = 0
  for (const item of items) {
    if (!('str' in item)) continue
    raw.push(item.str)
    for (let offset = 0; offset < item.str.length; offset++) {
      sources.push({
        start: { run, offset, textOffset: textOffset + offset },
        end: { run, offset: offset + 1, textOffset: textOffset + offset + 1 },
      })
    }
    textOffset += item.str.length
    if (item.hasEOL) {
      raw.push('\n')
      const at = { run, offset: item.str.length, textOffset }
      sources.push({ start: at, end: at })
    }
    run++
  }

  const characters: string[] = []
  const starts: PdfTextPosition[] = []
  const ends: PdfTextPosition[] = []
  // Include combining marks with their base, even across text-item boundaries.
  for (const cluster of raw.join('').matchAll(/\P{M}\p{M}*|\p{M}+/gu)) {
    const start = sources[cluster.index]!.start
    const end = sources[cluster.index + cluster[0].length - 1]!.end
    for (const character of normalise(cluster[0])) {
      const value = /\s/u.test(character) ? ' ' : character
      if (value === ' ' && characters[characters.length - 1] === ' ') {
        ends[ends.length - 1] = end
        continue
      }
      characters.push(value)
      for (let unit = 0; unit < value.length; unit++) {
        starts.push(start)
        ends.push(end)
      }
    }
  }
  return { text: characters.join(''), starts, ends }
}

export function findPdfMatches(
  page: IndexedPdfText,
  query: string,
  pageNumber: number,
): PdfSearchMatch[] {
  const needle = pdfSearchQuery(query)
  if (!needle) return []
  const matches: PdfSearchMatch[] = []
  let from = 0
  for (;;) {
    const at = page.text.indexOf(needle, from)
    if (at < 0) return matches
    const start = page.starts[at]!
    const end = page.ends[at + needle.length - 1]!
    const id = `${pageNumber}:${start.textOffset}:${end.textOffset}`
    // Searching "f" in "ﬀ" must not navigate to the same painted glyph twice.
    if (matches[matches.length - 1]?.id !== id) matches.push({ id, pageNumber, start, end })
    from = at + needle.length
  }
}

export const PDF_SEARCH_CONCURRENCY = 4

export class PdfTextIndex {
  readonly pages = new Map<number, IndexedPdfText>()
  readonly errors = new Map<number, Error>()
  private listeners = new Set<() => void>()
  private nextPage = 1
  private running = 0
  private version = 0

  constructor(private readonly doc: PdfSearchDocument) {}

  get loading(): boolean {
    return this.pages.size + this.errors.size < this.doc.numPages
  }

  getSnapshot = (): number => this.version

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    this.pump()
    return () => { this.listeners.delete(listener) }
  }

  private pump() {
    while (
      this.listeners.size && this.running < PDF_SEARCH_CONCURRENCY &&
      this.nextPage <= this.doc.numPages
    ) {
      const pageNumber = this.nextPage++
      this.running++
      void Promise.resolve()
        .then(() => this.doc.getPage(pageNumber))
        .then((page) => page.getTextContent())
        .then((content) => { this.pages.set(pageNumber, indexPdfText(content.items)) })
        .catch((cause: unknown) => {
          const detail = cause instanceof Error ? cause.message : String(cause ?? 'Unknown error')
          this.errors.set(pageNumber, new Error(`PDF search, page ${pageNumber}: ${detail}`))
        })
        .finally(() => {
          this.running--
          this.version++
          for (const listener of this.listeners) listener()
          // PDF.js text requests cannot be cancelled. Keep their result cached,
          // but do not start another page after the last reader unsubscribes.
          this.pump()
        })
    }
  }
}

const documents = new WeakMap<PdfSearchDocument, PdfTextIndex>()

export function pdfTextIndex(doc: PdfSearchDocument): PdfTextIndex {
  let index = documents.get(doc)
  if (!index) {
    index = new PdfTextIndex(doc)
    documents.set(doc, index)
  }
  return index
}
