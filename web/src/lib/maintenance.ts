/** The two long-running maintenance jobs, defined once.
 *
 *  Rebuilding the search index takes about thirty seconds on a hundred
 *  thousand items. It was wired in three places — the settings tab, the status
 *  tab and the command palette — each with its own copy of the messages, and
 *  none of them said anything while it ran. Three copies of a thing that is
 *  silent for half a minute is three chances to conclude the button is broken.
 */
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
