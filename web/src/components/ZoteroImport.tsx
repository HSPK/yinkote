import { useState } from 'react'

import { api } from '../api/client'
import type { ImportPreview } from '../api/types'
import { useT } from '../i18n'
import { useStore } from '../state/store'
import { Button, Input, toast } from '../ui'

/**
 * Bringing a Zotero library across.
 *
 * Two steps, deliberately: the counts appear first and nothing is written until
 * they are accepted. Merging one library into another is not something anybody
 * should discover they have done, and an undo for it does not exist.
 */
export function ZoteroImport() {
  const t = useT()
  const library = useStore((s) => s.library)
  const reloadSidebar = useStore((s) => s.reloadSidebar)
  const refresh = useStore((s) => s.refresh)

  const [path, setPath] = useState('')
  const [found, setFound] = useState<ImportPreview | null>(null)
  const [busy, setBusy] = useState(false)

  const look = async () => {
    setBusy(true)
    setFound(null)
    try {
      setFound(await api.import.preview(path.trim()))
    } catch (e) {
      toast.fromError(t('import.failed'), e)
    } finally {
      setBusy(false)
    }
  }

  const run = async () => {
    setBusy(true)
    try {
      const done = await api.import.zotero(library, path.trim())
      // Report what did not arrive as loudly as what did: a library quietly
      // missing a tenth of itself is found out much later, by its absence.
      const message = done.failed
        ? t('import.doneWithFailures', { items: done.items, failed: done.failed })
        : t('import.done', {
            items: done.items + done.updated,
            collections: done.collections,
            files: done.files,
          })
      toast.success(message)
      setFound(null)
      await Promise.all([refresh(), reloadSidebar()])
    } catch (e) {
      toast.fromError(t('import.failed'), e)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="import">
      <div className="import-row">
        <Input
          value={path}
          spellCheck={false}
          placeholder={t('import.pathPlaceholder')}
          onChange={(e) => {
            setPath(e.target.value)
            setFound(null)
          }}
        />
        <Button disabled={!path.trim() || busy} onClick={() => void look()}>
          {t('import.preview')}
        </Button>
      </div>

      {found && (
        <div className="import-found">
          <span>
            {t('import.found', {
              items: found.items,
              collections: found.collections,
              tags: found.tags,
            })}
          </span>
          <Button tone="primary" disabled={busy} onClick={() => void run()}>
            {busy ? t('import.working') : t('import.confirm')}
          </Button>
        </div>
      )}

      <p className="note">{t('import.note')}</p>
    </div>
  )
}
