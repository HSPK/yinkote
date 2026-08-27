/** Works the library keeps citing and does not hold.
 *
 *  The one view a citation graph exists to produce. Everything else in the
 *  workbench lists things the library *has*; this lists what it is standing on
 *  without owning — and a paper several of your own cite, that you have never
 *  read, is close to a definition of the next thing to read.
 *
 *  A list rather than a picture, because the question is ordered: which is
 *  cited most. A graph would show the same facts and answer it worse.
 */
import { useCallback, useEffect, useState } from 'react'

import { api } from '../api/client'
import type { MissingWork, Task } from '../api/types'
import { useT } from '../i18n'
import { useStore } from '../state/store'
import { Button, Empty, Icon, toast } from '../ui'
import { VirtualList } from '../components/VirtualList'
import { follow } from '../lib/tasks'

/** Narrower than this the columns scroll sideways rather than crush. */
const GAP_COLUMNS = 640

export function GapsPage() {
  const t = useT()
  const library = useStore((s) => s.library)
  const refresh = useStore((s) => s.refresh)
  const setGapCount = useStore((s) => s.setGapCount)

  const [works, setWorks] = useState<MissingWork[]>([])
  // The run, as the task registry sees it. It used to have a mechanism of its
  // own; there is one way of watching a long job now.
  const [harvest, setHarvest] = useState<Task | null>(null)
  const [loading, setLoading] = useState(true)
  const [adding, setAdding] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    const found = await api.references.missing(library).catch(() => ({ works: [] }))
    setWorks(found.works)
    setGapCount(found.works.length)
    setLoading(false)
  }, [library, setGapCount])

  useEffect(() => {
    void load()
  }, [load])

  // Pick up the last run — going, or finished before a reload. A job whose
  // progress nobody can see is one people start twice, and a run that has
  // already happened still has something worth saying: most publishers deposit
  // no references, so "stored 12, and 33 had none" is the difference between a
  // bug report and an explanation.
  useEffect(() => {
    let live = true
    void api.tasks
      .list()
      .then(({ tasks }) => {
        // Newest first, so the first harvest is the latest one.
        const last = tasks.find((t) => t.kind === 'harvest')
        if (!live || !last) return
        if (last.phase === 'running') void watch(last)
        else setHarvest(last)
      })
      .catch(() => {})
    return () => {
      live = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [library])

  /** Fetch the metadata for a DOI and file it, the same way quick-add does. */
  const add = async (work: MissingWork) => {
    const doi = work.doi
    if (!doi) return
    setAdding(work.fingerprint)
    try {
      const result = await api.scrape.quickAdd(library, { text: doi })
      if (!result.created?.length) throw new Error(t('gaps.notFound'))
      toast.success(t('gaps.added', { title: String(result.created[0]?.title ?? doi) }))
      // The row leaves this list because the library now holds it, which is the
      // only confirmation worth showing.
      await Promise.all([load(), refresh()])
    } catch (e) {
      toast.fromError(t('gaps.addFailed'), e)
    } finally {
      setAdding(null)
    }
  }

  /** Follow a run to the end, then refresh what it changed. */
  const watch = async (task: Task) => {
    setHarvest(task)
    const done = await follow(task.id, setHarvest)
    if (done) setHarvest(done)
    void load()
  }

  const running = harvest?.phase === 'running'
  /** Counters only this job has: how many lists were stored, how many
   *  publishers deposited none. */
  const counts = (harvest?.detail ?? {}) as { stored?: number; empty?: number }

  const bar = (
    <div className="gaps-bar">
      {running && harvest ? (
        <>
          <span>
            {t('gaps.harvesting', {
              done: harvest.done,
              total: harvest.total,
              stored: counts.stored ?? 0,
            })}
          </span>
          <Button onClick={() => void api.tasks.cancel(harvest.id).catch(() => {})}>
            {t('gaps.stop')}
          </Button>
        </>
      ) : (
        <>
          <span className="dim">
            {harvest && harvest.done > 0
              ? t('gaps.harvested', {
                  done: harvest.done,
                  stored: counts.stored ?? 0,
                  empty: counts.empty ?? 0,
                })
              : t('gaps.harvestHint')}
          </span>
          <Button
            tone="primary"
            onClick={() =>
              void api.references
                .startHarvest(library)
                .then(({ task }) => watch(task))
                .catch((e: unknown) => toast.fromError(t('gaps.harvestFailed'), e))
            }
          >
            {t('gaps.harvest')}
          </Button>
        </>
      )}
    </div>
  )

  if (loading) return <Empty>{t('gaps.loading')}</Empty>
  if (!works.length)
    return (
      <div className="pane main browser">
        {bar}
        <Empty>{t('gaps.none')}</Empty>
      </div>
    )

  return (
    <div className="pane main data-page">
      {bar}
      <VirtualList
        rows={works}
        keyOf={(work) => work.fingerprint}
        minWidth={GAP_COLUMNS}
        header={
          <div className="table-head gaps-grid">
            <div className="head-cell">{t('gaps.work')}</div>
            <div className="head-cell num">{t('gaps.year')}</div>
            <div className="head-cell num">{t('gaps.citedBy')}</div>
            <div className="head-cell" />
          </div>
        }
      >
        {(work) => (
          <div className="row browser-grid gaps-grid">
            <div className="cell name-cell" title={work.label}>
              <Icon.Graph className="glyph" />
              <span className="name">{work.label || work.doi}</span>
            </div>
            <div className="cell num dim">{work.year ?? ''}</div>
            <div className="cell num">{work.citedBy}</div>
            <div className="cell">
              <Button
                tone="primary"
                disabled={adding === work.fingerprint}
                onClick={() => void add(work)}
              >
                {adding === work.fingerprint ? t('gaps.adding') : t('gaps.add')}
              </Button>
            </div>
          </div>
        )}
      </VirtualList>
    </div>
  )
}
