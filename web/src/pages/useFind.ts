import { useCallback, useEffect, useRef, useState } from 'react'

import { occurrences, step } from '../lib/find'

/**
 * Find-in-document over an already-rendered text layer.
 *
 * Works on the DOM rather than on pdf.js's text content because the reader must
 * be able to *show* a match: the spans are where the coordinates already are,
 * so wrapping them needs no second coordinate system to keep in step with the
 * first.
 */
export function useFind(root: React.RefObject<HTMLElement | null>, query: string, ready: unknown) {
  const [total, setTotal] = useState(0)
  const [index, setIndex] = useState(0)
  const marks = useRef<HTMLElement[]>([])

  const clear = useCallback(() => {
    for (const span of root.current?.querySelectorAll<HTMLElement>('[data-found]') ?? []) {
      // Collapsing puts the split text nodes back, so repeated searches do not
      // shred the layer into ever smaller pieces.
      span.replaceWith(...span.childNodes)
      span.parentElement?.normalize()
    }
    marks.current = []
  }, [root])

  useEffect(() => {
    clear()
    const needle = query.trim()
    if (!needle || !root.current) {
      setTotal(0)
      return
    }

    const found: HTMLElement[] = []
    const walker = document.createTreeWalker(root.current, NodeFilter.SHOW_TEXT)
    const texts: Text[] = []
    while (walker.nextNode()) texts.push(walker.currentNode as Text)

    for (const node of texts) {
      const hits = occurrences(node.data, needle)
      // Right to left, so each split leaves the earlier offsets valid.
      for (const [start, end] of [...hits].reverse()) {
        const tail = node.splitText(start)
        tail.splitText(end - start)
        const mark = document.createElement('span')
        mark.dataset.found = 'true'
        tail.replaceWith(mark)
        mark.append(tail)
        found.unshift(mark)
      }
    }

    marks.current = found
    setTotal(found.length)
    setIndex(0)
    return clear
  }, [query, ready, root, clear])

  useEffect(() => {
    marks.current.forEach((mark, i) => {
      mark.dataset.current = i === index ? 'true' : 'false'
    })
    marks.current[index]?.scrollIntoView({ block: 'center', behavior: 'smooth' })
  }, [index, total])

  return {
    total,
    index: total ? index + 1 : 0,
    go: (delta: number) => setIndex((i) => step(i, marks.current.length, delta)),
  }
}
