import { useCallback, useEffect, useMemo, useState, useSyncExternalStore } from 'react'

import { step } from '../lib/find'
import {
  findPdfMatches,
  pdfSearchQuery,
  pdfTextIndex,
  type PdfSearchDocument,
  type PdfSearchMatch,
} from '../lib/pdf-search'

const idleSubscribe = () => () => {}
const idleSnapshot = () => 0

/** Search the document, not whichever text layers happen to be on screen. */
export function useFind(doc: PdfSearchDocument | null | undefined, query: string) {
  const needle = pdfSearchQuery(query)
  const source = useMemo(() => doc ? pdfTextIndex(doc) : null, [doc])
  const enabled = Boolean(source && needle)
  const version = useSyncExternalStore(
    enabled ? source!.subscribe : idleSubscribe,
    enabled ? source!.getSnapshot : idleSnapshot,
    idleSnapshot,
  )
  const pageMatches = useMemo(() => new Map<number, PdfSearchMatch[]>(), [source, needle])
  const { matches, matchesByPage } = useMemo(() => {
    const byPage = new Map<number, PdfSearchMatch[]>()
    if (source && needle) {
      for (const pageNumber of [...source.pages.keys()].sort((a, b) => a - b)) {
        let found = pageMatches.get(pageNumber)
        if (!found) {
          found = findPdfMatches(source.pages.get(pageNumber)!, needle, pageNumber)
          pageMatches.set(pageNumber, found)
        }
        if (found.length) byPage.set(pageNumber, found)
      }
    }
    return { matches: [...byPage.values()].flat(), matchesByPage: byPage }
  }, [source, needle, pageMatches, version])
  const [selection, setSelection] = useState<{
    source: typeof source
    needle: string
    id: string
  } | null>(null)
  const selected = selection?.source === source && selection.needle === needle
    ? matches.findIndex((match) => match.id === selection.id) : -1
  const activeIndex = selected < 0 ? 0 : selected
  const active = matches[activeIndex] ?? null

  useEffect(() => {
    if (!active) {
      setSelection(null)
    } else if (selected < 0) {
      // Keep the same target when an earlier page finishes indexing later.
      setSelection({ source, needle, id: active.id })
    }
  }, [active, selected, source, needle])

  const go = useCallback((delta: number) => {
    if (!matches.length) return
    setSelection((previous) => {
      const current = previous?.source === source && previous.needle === needle
        ? matches.findIndex((match) => match.id === previous.id) : -1
      const next = matches[step(current < 0 ? 0 : current, matches.length, delta)]!
      return { source, needle, id: next.id }
    })
  }, [matches, source, needle])

  return {
    total: matches.length,
    index: active ? activeIndex + 1 : 0,
    go,
    loading: enabled && source!.loading,
    error: enabled ? source!.errors.values().next().value ?? null : null,
    matchesByPage,
    active,
  }
}
