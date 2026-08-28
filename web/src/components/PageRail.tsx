import { useEffect, useRef, useState } from 'react'

import { useT } from '../i18n'
import { Thumbnail } from './Thumbnail'

/**
 * A rail of page thumbnails down the side of the reader.
 *
 * The fastest way to find a figure in a forty-page paper, and the reason the
 * server grew a thumbnail cache: the pictures are drawn once, by whichever
 * browser opens the document first, and every later reader gets a static file.
 *
 * Only what is on screen is asked for. A three-hundred-page thesis would
 * otherwise render three hundred pages before showing the first one — and each
 * miss is a full pdf.js page render, so the cost of being eager here is not the
 * usual "a few extra requests".
 */
export function PageRail({
  library,
  attachmentKey,
  pages,
  current,
  onJump,
}: {
  library: number
  attachmentKey: string
  pages: number[]
  current: number
  onJump: (page: number) => void
}) {
  const t = useT()
  const railRef = useRef<HTMLDivElement>(null)
  const [visible, setVisible] = useState<Set<number>>(new Set())

  useEffect(() => {
    const root = railRef.current
    if (!root) return
    const observer = new IntersectionObserver(
      (entries) => {
        setVisible((was) => {
          const now = new Set(was)
          for (const entry of entries) {
            const page = Number((entry.target as HTMLElement).dataset.page)
            // Once drawn, kept: scrolling back up should not redraw what the
            // browser already has, and the set is bounded by the page count.
            if (entry.isIntersecting) now.add(page)
          }
          return now
        })
      },
      { root, rootMargin: '200px 0px' },
    )
    root.querySelectorAll('[data-page]').forEach((cell) => observer.observe(cell))
    return () => observer.disconnect()
  }, [pages.length])

  // Follow the reader: scrolling the document moves the rail, so the current
  // page is always in view without the user having to hunt for it.
  useEffect(() => {
    railRef.current
      ?.querySelector(`[data-page="${current}"]`)
      ?.scrollIntoView({ block: 'nearest' })
  }, [current])

  return (
    <div className="page-rail" ref={railRef} aria-label={t('reader.pages')}>
      {pages.map((page) => (
        <button
          key={page}
          className="page-cell"
          data-page={page}
          data-active={page === current}
          onClick={() => onJump(page)}
          title={t('reader.goToPage', { page })}
        >
          <span className="page-shot">
            {visible.has(page) && (
              <Thumbnail
                library={library}
                attachmentKey={attachmentKey}
                page={page}
                width={96}
              />
            )}
          </span>
          <span className="page-number">{page}</span>
        </button>
      ))}
    </div>
  )
}
