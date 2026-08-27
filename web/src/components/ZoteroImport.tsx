import { useState } from 'react'

import { api } from '../api/client'
import type { ImportPreview, ImportResult } from '../api/types'
import { useT } from '../i18n'
import { useStore } from '../state/store'
import { follow, percentOf } from '../lib/tasks'
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
  /** What the run is doing, while it does it. */
  const [progress, setProgress] = useState<string | null>(null)

  const look = async () => {
    setBusy(true)
    setFound(null)
    try {
      setFound(await api.import.preview(path.trim()))
    } catch (e) {
      toast.fromError(t('import.failed'), e)
    } finally {
      setBusy(false)
      setProgress(null)
    }
  }

  const run = async () => {
    setBusy(true)
    try {
      const { task } = await api.import.zotero(library, path.trim())
      // Watched, not awaited: a real Zotero library takes minutes, and this is
      // the first thing somebody does with the program.
      const state = await follow(task.id, (t) => {
        const pct = percentOf(t)
        setProgress(pct === null ? t.message : `${t.message} · ${pct}%`)
      })
      setProgress(null)
      if (!state || state.phase === 'failed') {
        throw new Error(state?.error ?? t('toast.taskLost'))
      }
      const done = state.result as unknown as ImportResult
      // Report what did not arrive as loudly as what did: a library quietly
      // missing a tenth of itself is found out much later, by its absence.
      const message = done.failed
        ? t('import.doneWithFailures', { items: done.items, failed: done.failed })
        : t('import.done', {
            items: done.items + done.updated,
            files: done.files,
            notes: done.notes,
            annotations: done.annotations,
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
              attachments: found.attachments,
              notes: found.notes,
              annotations: found.annotations,
            })}
          </span>
          <Button tone="primary" disabled={busy} onClick={() => void run()}>
            {busy ? (progress ?? t('import.working')) : t('import.confirm')}
          </Button>
        </div>
      )}

      <p className="note">{t('import.note')}</p>
    </div>
  )
}
