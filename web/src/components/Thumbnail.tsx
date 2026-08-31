import { useEffect, useState } from 'react'

import { useT } from '../i18n'
import { THUMB_WIDTHS, thumbnailFor, type ThumbWidth } from '../lib/thumbnails'

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
/** The cached size to ask for, given how big it will be drawn.
 *
 *  Only the three the server will cache are allowed, so this snaps up to the
 *  first that covers the device pixels rather than asking for an arbitrary
 *  number and being refused. */
function cacheWidth(width: ThumbWidth): ThumbWidth {
  const dpr = typeof window === 'undefined' ? 1 : window.devicePixelRatio || 1
  const wanted = width * dpr
  return THUMB_WIDTHS.find((w) => w >= wanted) ?? 960
}

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

  // `width` is the size on screen; the picture has to be that many *device*
  // pixels or it is resampled up and the page reads as a blur. A 240-wide
  // bitmap shown at 240 CSS pixels is half resolution on any screen made in
  // the last decade, which is exactly what the cover looked like.
  const cached = cacheWidth(width)

  useEffect(() => {
    let live = true
    let objectUrl: string | null = null

    void thumbnailFor(library, attachmentKey, page, cached).then((blob) => {
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
  }, [library, attachmentKey, page, cached])

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
