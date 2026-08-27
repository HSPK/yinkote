/** Watching a job the server is running.
 *
 *  The server hands back a handle rather than holding the request open, so
 *  something has to watch it. That something is here once: three pages needing
 *  a progress bar is three chances to write a polling loop that forgets to
 *  stop.
 */
import { api } from '../api/client'
import type { Task } from '../api/types'

/** How often to ask. Long jobs are minutes; a second is responsive enough and
 *  costs a request nobody notices. */
const POLL_MS = 1000

/** Follow a task until it stops, reporting each state on the way.
 *
 *  Resolves with the final state — including a failure, which is a result and
 *  not an exception: the caller wants to say *what* went wrong, and a rejected
 *  promise makes that harder rather than easier.
 */
export async function follow(
  id: string,
  onProgress?: (task: Task) => void,
  signal?: { cancelled: boolean },
): Promise<Task | null> {
  for (;;) {
    if (signal?.cancelled) return null
    const task = await api.tasks.get(id).catch(() => null)
    // A task the server has forgotten is not an error worth throwing: it
    // finished long enough ago to be pruned, and there is nothing to report.
    if (!task) return null
    onProgress?.(task)
    if (task.phase !== 'running') return task
    await new Promise((r) => setTimeout(r, POLL_MS))
  }
}

/** What to show while a job runs: a fraction when it knows, otherwise nothing.
 *
 *  A bar that invents a percentage is worse than a spinner, because it will be
 *  believed. `total === 0` is the server saying it cannot count.
 */
export function percentOf(task: Task): number | null {
  if (task.total <= 0) return null
  return Math.min(100, Math.round((task.done / task.total) * 100))
}
