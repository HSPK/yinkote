/** What the server has been asked to do.
 *
 *  Long jobs outlive the request that started them, the toast that announced
 *  them and the tab somebody was looking at. The status bar says *that*
 *  something is running; this says what, how far, how it ended, and what it
 *  produced — which is the question somebody actually has an hour later
 *  ("where did that export go?").
 *
 *  A table, like the downloads queue and for the same reason: a header naming
 *  the columns, rows under it, and exactly one thing that scrolls.
 */
import { useCallback, useEffect, useState } from 'react'
import { taskMessage } from '../lib/format'

import { api } from '../api/client'
import type { Task } from '../api/types'
import { VirtualList } from '../components/VirtualList'
import { useT } from '../i18n'
import { humanBytes } from '../lib/maintenance'
import { percentOf } from '../lib/tasks'
import { Button, Empty, toast } from '../ui'

/** Narrower than this the columns scroll sideways rather than crush. */
const TASK_COLUMNS = 760

/** Whether two polls describe the same jobs.
 *
 *  Compares only what is drawn: progress ticks while a job runs and the row
 *  must follow, but an unchanged list must not produce a new array, which
 *  would re-render every row for no visible change.
 */
function sameTasks(a: Task[], b: Task[]): boolean {
  if (a.length !== b.length) return false
  return a.every((row, i) => {
    const other = b[i]
    return (
      !!other &&
      row.id === other.id &&
      row.phase === other.phase &&
      row.done === other.done &&
      row.message === other.message
    )
  })
}

/** What a finished job produced, in one line.
 *
 *  Each kind returns its own shape, and the useful part differs: an export is
 *  a file somebody has to find, an import is a count of what arrived. Reading
 *  the shape here keeps that knowledge in one place instead of asking every
 *  job to phrase its own summary in English the interface cannot re-translate.
 */
function outcome(task: Task, t: ReturnType<typeof useT>): string {
  if (task.phase === 'failed') return task.error ?? t('tasks.failed')
  const r = task.result
  if (!r) return ''
  if (typeof r.name === 'string') {
    return `${r.name}${typeof r.bytes === 'number' ? ` · ${humanBytes(r.bytes)}` : ''}`
  }
  if (typeof r.items === 'number' || typeof r.skipped === 'number') {
    return t('tasks.imported', {
      items: Number(r.items ?? 0),
      skipped: Number(r.skipped ?? 0),
    })
  }
  if (typeof r.reindexed === 'number') {
    return t('tasks.reindexed', { count: Number(r.reindexed) })
  }
  if (typeof r.stored === 'number') {
    return t('tasks.stored', { count: Number(r.stored) })
  }
  return ''
}

export function TasksPage() {
  const t = useT()
  const [rows, setRows] = useState<Task[]>([])
  const [loading, setLoading] = useState(true)

  const load = useCallback(async () => {
    const found = await api.tasks.list().catch(() => null)
    if (found) setRows((current) => (sameTasks(current, found.tasks) ? current : found.tasks))
    setLoading(false)
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const busy = rows.some((r) => r.phase === 'running')
  useEffect(() => {
    // Faster while something is going, slow while nothing is: this page may be
    // left open, and a finished list does not change on its own.
    const timer = window.setInterval(() => void load(), busy ? 1000 : 6000)
    return () => window.clearInterval(timer)
  }, [busy, load])

  const header = (
    <div className="table-head tasks-grid">
      <div className="head-cell">{t('tasks.col.job')}</div>
      <div className="head-cell">{t('tasks.col.state')}</div>
      <div className="head-cell">{t('tasks.col.outcome')}</div>
      <div className="head-cell num">{t('tasks.col.started')}</div>
      <div className="head-cell" />
    </div>
  )

  if (loading) return <Empty>{t('tasks.loading')}</Empty>
  if (!rows.length) return <Empty>{t('tasks.none')}</Empty>

  return (
    <div className="pane main data-page">
      <VirtualList rows={rows} keyOf={(task) => task.id} minWidth={TASK_COLUMNS} header={header}>
        {(task) => {
          const pct = percentOf(task)
          return (
            <div className="row tasks-grid" data-phase={task.phase}>
              <div className="cell">{t(`tasks.kind.${task.kind}` as never) || task.kind}</div>
              <div className="cell dim">
                {task.phase === 'running'
                  ? `${taskMessage(t, task.message)}${pct === null ? '' : ` · ${pct}%`}`
                  : t(`tasks.phase.${task.phase}` as never)}
              </div>
              <div className="cell dim" title={outcome(task, t)}>
                {outcome(task, t)}
              </div>
              <div className="cell num dim">{when(task.startedAt)}</div>
              <div className="cell">
                {task.phase === 'running' && (
                  <Button
                    tone="ghost"
                    onClick={() =>
                      void api.tasks
                        .cancel(task.id)
                        .then(() => load())
                        .catch((e: unknown) => toast.fromError(t('tasks.cancelFailed'), e))
                    }
                  >
                    {t('tasks.stop')}
                  </Button>
                )}
              </div>
            </div>
          )
        }}
      </VirtualList>
    </div>
  )
}

/** When a job started, at the resolution that distinguishes two of them. */
function when(seconds: number): string {
  if (!seconds) return ''
  const at = new Date(seconds * 1000)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}`
}
