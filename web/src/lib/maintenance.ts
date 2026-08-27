/** The two long-running maintenance jobs, defined once.
 *
 *  Rebuilding the search index takes about thirty seconds on a hundred
 *  thousand items. It was wired in three places — the settings tab, the status
 *  tab and the command palette — each with its own copy of the messages, and
 *  none of them said anything while it ran. Three copies of a thing that is
 *  silent for half a minute is three chances to conclude the button is broken.
 */
import { api } from '../api/client'
import { follow } from './tasks'
import { t } from '../i18n'
import { useStore } from '../state/store'
import { withToast } from '../ui'

/** Rebuild the search index. Slow: it reads and re-indexes every item. */
export async function runReindex(): Promise<void> {
  await withToast(useStore.getState().reindex, {
    pending: t('toast.reindexing'),
    success: t('toast.reindexed'),
    failure: t('toast.reindexFailed'),
  })
}

/** Compact the database. Usually quick, occasionally not. */
export async function runOptimize(): Promise<void> {
  await withToast(useStore.getState().optimize, {
    pending: t('toast.optimizing'),
    success: t('toast.optimized'),
    failure: t('toast.optimizeFailed'),
  })
}

/** Take a backup now. Slow in proportion to the library: a 300MB one takes
 *  about four seconds, which is long enough to need saying so. */
export async function runBackup(): Promise<void> {
  await withToast(
    async () => {
      const made = await api.maintenance.backup()
      return made
    },
    {
      pending: t('toast.backingUp'),
      success: (made) =>
        t('toast.backedUp', { name: made.name, size: humanBytes(made.bytes) }),
      failure: t('toast.backupFailed'),
    },
  )
}

/** Check that the database and the disk still agree. Reports; never repairs. */
export async function runIntegrity(): Promise<void> {
  await withToast(async () => await api.maintenance.integrity(), {
    pending: t('toast.checking'),
    success: (report) =>
      report.missing.length === 0 && report.orphans.length === 0
        ? t('toast.integrityClean', { n: report.checked })
        : t('toast.integrityFound', {
            missing: report.missing.length,
            orphans: report.orphans.length,
          }),
    failure: t('toast.integrityFailed'),
  })
}

/** Bytes at the resolution a person reads them: nobody wants 319459328. */
export function humanBytes(bytes: number): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let n = bytes
  let unit = 0
  while (n >= 1024 && unit < units.length - 1) {
    n /= 1024
    unit += 1
  }
  return `${n < 10 && unit > 0 ? n.toFixed(1) : Math.round(n)} ${units[unit]}`
}

/** Pack the whole library — database and files — into one movable archive.
 *
 *  The server runs this as a task, so this waits on the task rather than on the
 *  request: a large library takes minutes, and a request held open that long is
 *  a client with nothing to show and a proxy free to give up.
 */
export async function runExportAll(): Promise<void> {
  await withToast(
    async () => {
      const { task } = await api.maintenance.exportAll()
      const done = await follow(task.id)
      if (!done || done.phase !== 'done') throw new Error(done?.error ?? t('toast.taskLost'))
      return done.result as { name: string; bytes: number }
    },
    {
      pending: t('toast.exportingAll'),
      success: (made) =>
        t('toast.exportedAll', {
          name: String(made?.name ?? ''),
          size: humanBytes(Number(made?.bytes ?? 0)),
        }),
      failure: t('toast.exportAllFailed'),
    },
  )
}
