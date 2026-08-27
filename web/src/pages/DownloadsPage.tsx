/** The download queue.
 *
 *  A surface rather than a toast, because a download is not an event: it takes
 *  time, it fails for reasons worth reading, and the decision it needs — retry,
 *  give up, paste a different address — comes later than the moment it broke.
 *
 *  Rows that need a decision sort to the top. Everything else is history, and
 *  history is what the clear button is for.
 */
import { useCallback, useEffect, useState } from 'react'

import { api } from '../api/client'
import type { Download } from '../api/types'
import { useT } from '../i18n'
import { bytes as formatBytes } from '../lib/format'
import { useStore } from '../state/store'
import { Button, Empty, Icon, toast } from '../ui'
import { VirtualList } from '../components/VirtualList'

/** Narrower than this the columns scroll sideways rather than crush. */
const DOWNLOAD_COLUMNS = 880

/** Whether two polls describe the same queue.
 *
 *  Compares only what is drawn. Bytes tick up while a file is downloading and
 *  the row must follow, but an identical queue must not produce a new array —
 *  that is a re-render of every row for no visible change.
 */
function sameQueue(a: Download[], b: Download[]): boolean {
  if (a.length !== b.length) return false
  return a.every((row, i) => {
    const other = b[i]
    return (
      !!other &&
      row.id === other.id &&
      row.state === other.state &&
      row.bytes === other.bytes &&
      row.error === other.error
    )
  })
}

export function DownloadsPage() {
  const t = useT()
  const library = useStore((s) => s.library)
  const setDownloadCount = useStore((s) => s.setDownloadCount)

  const [rows, setRows] = useState<Download[]>([])
  const [loading, setLoading] = useState(true)
  /** Whether anything is still moving; see the polling effect below. */
  const [busy, setBusy] = useState(true)

  const load = useCallback(async () => {
    const found = await api.downloads.list(library).catch(() => null)
    if (found) {
      // Replaced only when something actually changed. A poll that hands back
      // a fresh array every time re-renders every row for nothing, which is
      // what made a settled queue feel busy.
      setRows((current) => (sameQueue(current, found.downloads) ? current : found.downloads))
      setDownloadCount(found.waiting + found.failed)
      setBusy(found.waiting > 0 || found.downloads.some((d) => d.state === 'running'))
    }
    setLoading(false)
  }, [library, setDownloadCount])

  useEffect(() => {
    void load()
  }, [load])

  useEffect(() => {
    // Polled only while something is outstanding. The worker takes one file at
    // a time, so a second and a half is far finer than the queue moves — and a
    // queue that has finished does not move at all.
    if (!busy) return
    const timer = window.setInterval(() => void load(), 1500)
    return () => window.clearInterval(timer)
  }, [busy, load])

  const act = async (what: 'retry' | 'remove', ids: number[]) => {
    try {
      await (what === 'retry'
        ? api.downloads.retry(library, ids)
        : api.downloads.remove(library, ids))
      await load()
    } catch (e) {
      toast.fromError(t('downloads.actionFailed'), e)
    }
  }

  if (loading) return <Empty>{t('downloads.loading')}</Empty>

  const failed = rows.filter((r) => r.state === 'failed').map((r) => r.id)

  return (
    <div className="pane main browser">
      <div className="gaps-bar">
        <span className="dim">
          {t('downloads.summary', {
            waiting: rows.filter((r) => r.state === 'waiting').length,
            failed: failed.length,
            done: rows.filter((r) => r.state === 'done').length,
          })}
        </span>
        <span className="row-actions">
          <Button disabled={!failed.length} onClick={() => void act('retry', failed)}>
            {t('downloads.retryAll')}
          </Button>
          <Button
            onClick={() =>
              void api.downloads
                .clear(library)
                .then(load)
                .catch((e: unknown) => toast.fromError(t('downloads.actionFailed'), e))
            }
          >
            {t('downloads.clear')}
          </Button>
        </span>
      </div>

      <VirtualList
        rows={rows}
        keyOf={(row) => String(row.id)}
        minWidth={DOWNLOAD_COLUMNS}
        empty={<Empty>{t('downloads.none')}</Empty>}
      >
        {(row) => (
          <div className="row browser-grid downloads-grid" data-state={row.state}>
            <div className="cell name-cell" title={row.url}>
              <Icon.Download className="glyph" />
              <span className="name">{row.title || row.url}</span>
            </div>
            <div className="cell dim mono" title={row.url}>
              {row.url}
            </div>
            <div className="cell state-cell">
              <span className="download-state" data-state={row.state}>
                {t(`downloads.state.${row.state}`)}
              </span>
              {/* The reason lives beside the row, not in a log: it is what the
                  decision to retry is made from. One line, full text on hover —
                  a message that wraps breaks the rhythm of every row under it. */}
              {row.error && (
                <span className="download-error" title={row.error}>
                  {row.error}
                </span>
              )}
            </div>
            <div className="cell num dim">{row.bytes ? formatBytes(row.bytes) : ''}</div>
            <div className="cell row-actions">
              {row.state === 'failed' && (
                <Button onClick={() => void act('retry', [row.id])}>{t('downloads.retry')}</Button>
              )}
              <Button tone="ghost" onClick={() => void act('remove', [row.id])}>
                {t('downloads.remove')}
              </Button>
            </div>
          </div>
        )}
      </VirtualList>
    </div>
  )
}
