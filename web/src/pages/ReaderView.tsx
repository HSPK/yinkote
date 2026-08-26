import { useEffect, useState } from 'react'

import { api } from '../api/client'
import type { Item } from '../api/types'
import { useT } from '../i18n'
import { useStore } from '../state/store'
import { Empty } from '../ui'

/**
 * Reads an item's attached PDF.
 *
 * A tab rather than a modal because reading is not a detour: it is the thing
 * the library exists for, and it must be possible to search, chat and annotate
 * without losing your place.
 */
export function ReaderView({ target }: { target?: string }) {
  const t = useT()
  const library = useStore((s) => s.library)
  const [attachments, setAttachments] = useState<Item[]>([])
  const [current, setCurrent] = useState<string | null>(null)

  useEffect(() => {
    if (!target) return
    let live = true
    api.items
      .children(library, target)
      .then((kids) => {
        if (!live) return
        const files = kids.filter((k) => k.itemType === 'attachment')
        setAttachments(files)
        setCurrent((c) => c ?? files[0]?.key ?? null)
      })
      .catch(() => setAttachments([]))
    return () => {
      live = false
    }
  }, [library, target])

  if (!target) return <Empty>{t('reader.none')}</Empty>
  if (!attachments.length) return <Empty>{t('reader.noFile')}</Empty>

  return (
    <div className="pane main reader">
      {attachments.length > 1 && (
        <div className="reader-files">
          {attachments.map((file) => (
            <button
              key={file.key}
              data-active={file.key === current}
              onClick={() => setCurrent(file.key)}
            >
              {String(file.title ?? file.filename ?? file.key)}
            </button>
          ))}
        </div>
      )}
      {current && (
        <object
          className="reader-frame"
          data={api.files.url(library, current)}
          type="application/pdf"
        >
          <Empty>{t('reader.unsupported')}</Empty>
        </object>
      )}
    </div>
  )
}
