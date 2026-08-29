/** Reading a `.yinkote` archive back in.
 *
 *  A path rather than an upload, like the Zotero import and for the same
 *  reason: the archive is hundreds of megabytes and it is already on the
 *  machine the server is running on. Pushing it through the browser would copy
 *  it twice for no benefit.
 *
 *  Merging, never replacing — so this needs no confirmation step. Anything
 *  already in the library is left exactly as it is.
 */
import { useState } from 'react'
import { taskMessage } from '../lib/format'

import { api } from '../api/client'
import { useT } from '../i18n'
import { useStore } from '../state/store'
import { follow, percentOf } from '../lib/tasks'
import { Button, Input, toast } from '../ui'

export function ArchiveImport() {
  const t = useT()
  const refresh = useStore((s) => s.refresh)
  const reloadSidebar = useStore((s) => s.reloadSidebar)

  const [path, setPath] = useState('')
  const [busy, setBusy] = useState(false)
  const [report, setReport] = useState<string | null>(null)

  /** What a running job should say. A percentage only when it knows one. */
  const progressText = (task: import('../api/types').Task) => {
    const pct = percentOf(task)
    return pct === null ? taskMessage(t, task.message) : `${taskMessage(t, task.message)} · ${pct}%`
  }

  const run = async () => {
    setBusy(true)
    setReport(null)
    try {
      const { task } = await api.maintenance.importArchive(path.trim())
      // Watched rather than awaited, and reported as it goes: an import of a
      // real library is minutes of silence otherwise.
      const done = await follow(task.id, (t) => setReport(progressText(t)))
      if (!done || done.phase !== 'done') throw new Error(done?.error ?? t('toast.taskLost'))
      const r = done.result as { items: number; skipped: number; files: number }
      setReport(
        t('import.archiveDone', { items: r.items, skipped: r.skipped, files: r.files }),
      )
      await refresh()
      await reloadSidebar()
    } catch (e) {
      toast.fromError(t('import.failed'), e)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="import-row">
      <Input
        value={path}
        placeholder={t('import.archivePath')}
        onChange={(e) => setPath(e.target.value)}
      />
      <Button disabled={busy || !path.trim()} onClick={() => void run()}>
        {busy ? t('import.archiveReading') : t('import.archiveRun')}
      </Button>
      {report && <span className="dim">{report}</span>}
    </div>
  )
}
