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
import type { MissingWork } from '../api/types'
import { useT } from '../i18n'
import { useStore } from '../state/store'
import { Button, Empty, Icon, toast } from '../ui'

export function GapsPage() {
  const t = useT()
  const library = useStore((s) => s.library)
  const refresh = useStore((s) => s.refresh)
  const setGapCount = useStore((s) => s.setGapCount)

  const [works, setWorks] = useState<MissingWork[]>([])
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

  if (loading) return <Empty>{t('gaps.loading')}</Empty>
  if (!works.length) return <Empty>{t('gaps.none')}</Empty>

  return (
    <div className="pane main browser">
      <div className="browser-head browser-grid gaps-grid">
        <span>{t('gaps.work')}</span>
        <span className="num">{t('gaps.year')}</span>
        <span className="num">{t('gaps.citedBy')}</span>
        <span />
      </div>

      <div className="browser-body">
        {works.map((work) => (
          <div key={work.fingerprint} className="row browser-grid gaps-grid">
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
        ))}
      </div>
    </div>
  )
}
