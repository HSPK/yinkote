/** What the server is busy with.
 *
 *  A job now outlives the request that started it, which means it also outlives
 *  the toast that announced it and the tab somebody happened to be looking at.
 *  Without somewhere permanent to show it, "I started an export" becomes "did
 *  I start an export?" — and the answer is a reload away from being lost.
 *
 *  Only while something is running. A permanent "0 jobs" is noise.
 */
import { useEffect, useState } from 'react'
import { taskMessage } from '../lib/format'

import { api } from '../api/client'
import type { Task } from '../api/types'
import { useT } from '../i18n'
import { percentOf } from '../lib/tasks'

/** How often to ask while something is running.
 *
 *  Slower than the follower a page uses, because this is ambient: it says
 *  *that* something is happening, and whoever wants detail is watching the page
 *  that started it.
 */
const POLL_MS = 2000

/** And how often to check whether anything has started elsewhere. */
const IDLE_POLL_MS = 8000

export function ActivityIndicator() {
  const t = useT()
  const [running, setRunning] = useState<Task[]>([])

  useEffect(() => {
    let live = true
    let timer = 0

    const tick = async () => {
      const found = await api.tasks.list().catch(() => null)
      if (!live) return
      const busy = found?.tasks.filter((task) => task.phase === 'running') ?? []
      setRunning(busy)
      // Ask more often while there is something to watch, and back off when
      // there is not: this runs for as long as the program is open.
      timer = window.setTimeout(tick, busy.length ? POLL_MS : IDLE_POLL_MS)
    }
    void tick()

    return () => {
      live = false
      window.clearTimeout(timer)
    }
  }, [])

  if (!running.length) return null

  const first = running[0]!
  const pct = percentOf(first)
  const label = running.length > 1 ? t('tasks.several', { count: running.length }) : first.message

  return (
    <span
      className="activity"
      title={running.map((task) => `${task.kind}: ${taskMessage(t, task.message)}`).join('\n')}
    >
      <span className="activity-spin" />
      <span>{label}</span>
      {pct !== null && <span className="dim">{pct}%</span>}
      <button
        className="activity-stop"
        title={t('tasks.cancel')}
        onClick={() => void api.tasks.cancel(first.id).catch(() => {})}
      >
        {t('tasks.stop')}
      </button>
    </span>
  )
}
