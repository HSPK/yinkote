import { useEffect, useState } from 'react'

import { useT } from '../i18n'
import { thumbnailFor, type ThumbWidth } from '../lib/thumbnails'

/**
 * A page of a PDF, as a picture.
 *
 * Cheap when cached — the server answers with a static file — and only
 * expensive the first time, when the browser draws the page and hands it back.
 * See `lib/thumbnails.ts` for why the split falls that way.
 *
 * Renders nothing at all when there is no picture to show. Most items in a
 * library have no PDF, and an error box or a broken-image glyph on every one of
 * them would be noise reporting the ordinary case.
 */
export function Thumbnail({
  library,
  attachmentKey,
  page = 1,
  width = 240,
  className,
}: {
  library: number
  attachmentKey: string
  page?: number
  width?: ThumbWidth
  className?: string
}) {
  const t = useT()
  const [url, setUrl] = useState<string | null>(null)

  useEffect(() => {
    let live = true
    let objectUrl: string | null = null

    void thumbnailFor(library, attachmentKey, page, width).then((blob) => {
      if (!live || !blob) return
      objectUrl = URL.createObjectURL(blob)
      setUrl(objectUrl)
    })

    return () => {
      live = false
      setUrl(null)
      // Object URLs hold their blob alive until revoked, and a table that
      // scrolls past a thousand covers would hold a thousand page images.
      if (objectUrl) URL.revokeObjectURL(objectUrl)
    }
  }, [library, attachmentKey, page, width])

  if (!url) return null

  return (
    <img
      className={className ?? 'thumbnail'}
      src={url}
      alt={t('detail.thumbnailAlt', { page })}
      loading="lazy"
      draggable={false}
    />
  )
}
