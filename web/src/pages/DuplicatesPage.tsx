/** The same paper, filed twice.
 *
 *  Every library that has ever imported from two places has these, and they are
 *  invisible from the item list: the two records sit pages apart under slightly
 *  different titles. So this is a screen of its own, and the only screen whose
 *  job is to make itself empty.
 *
 *  Merging is the one action in the workbench that a user cannot undo by hand,
 *  which is why the losing records go to the trash rather than being destroyed,
 *  and why each group shows what is actually on each copy — attachments, tags,
 *  which collection it is filed in — before anyone chooses.
 */
import { useCallback, useEffect, useState } from 'react'

import { api } from '../api/client'
import type { Item } from '../api/types'
import { useT } from '../i18n'
import { creatorSummary, shortDate, year } from '../lib/format'
import { useStore } from '../state/store'
import { Button, Empty, Icon, toast } from '../ui'

export function DuplicatesPage() {
  const t = useT()
  const library = useStore((s) => s.library)
  const refresh = useStore((s) => s.refresh)

  const [groups, setGroups] = useState<Item[][]>([])
  const [loading, setLoading] = useState(true)
  const [merging, setMerging] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    const found = await api.duplicates.groups(library).catch(() => ({ groups: [], total: 0 }))
    setGroups(found.groups)
    setLoading(false)
  }, [library])

  useEffect(() => {
    void load()
  }, [load])

  const merge = async (group: Item[], master: Item) => {
    setMerging(master.key)
    const others = group.filter((i) => i.key !== master.key).map((i) => i.key)
    try {
      await api.duplicates.merge(library, master.key, others)
      toast.success(t('duplicates.merged', { n: others.length }))
      // The group is gone from the library, so take it off the screen rather
      // than reloading everything: the rest of the list is still true.
      setGroups((gs) => gs.filter((g) => g !== group))
      void refresh()
    } catch (e: unknown) {
      toast.fromError(t('duplicates.mergeFailed'), e)
    } finally {
      setMerging(null)
    }
  }

  const bar = (
    <div className="gaps-bar">
      <span className="dim">
        {loading ? t('duplicates.loading') : t('duplicates.count', { n: groups.length })}
      </span>
      <Button onClick={() => void load()} disabled={loading}>
        {t('duplicates.rescan')}
      </Button>
    </div>
  )

  if (loading) return <Empty>{t('duplicates.loading')}</Empty>
  if (!groups.length)
    return (
      <div className="pane main browser">
        {bar}
        <Empty>{t('duplicates.none')}</Empty>
      </div>
    )

  return (
    <div className="pane main data-page">
      {bar}
      <div className="dup-scroll">
        {groups.map((group) => (
          <div className="dup-group" key={group.map((i) => i.key).join('-')}>
            <div className="dup-group-head">
              <span className="dup-group-title">{group[0]?.title ?? t('detail.untitled')}</span>
              <span className="dup-group-note">{t('duplicates.copies', { n: group.length })}</span>
            </div>
            {group.map((item) => (
              <div className="dup-row" key={item.key}>
                <span className="dup-cell dim">{creatorSummary(item)}</span>
                <span className="dup-cell num">{year(item)}</span>
                <span className="dup-cell dim">{String(item.publicationTitle ?? '')}</span>
                {/* What is only on this copy is exactly what decides which to
                    keep, so it goes on the row rather than a click away. */}
                <span className="dup-cell dup-marks">
                  {(item.attachments?.length ?? 0) > 0 && (
                    <span className="attach-mark" title={t('table.attachments')}>
                      <Icon.Paperclip size={12} />
                    </span>
                  )}
                  {item.tags.length > 0 && (
                    <span className="dup-badge">
                      {t('duplicates.tags', { n: item.tags.length })}
                    </span>
                  )}
                  {item.collections.length > 0 && (
                    <span className="dup-badge">
                      {t('duplicates.collections', { n: item.collections.length })}
                    </span>
                  )}
                </span>
                <span className="dup-cell num">{shortDate(item.dateAdded)}</span>
                <Button
                  tone="ghost"
                  disabled={merging !== null}
                  title={t('duplicates.keepHint')}
                  onClick={() => void merge(group, item)}
                >
                  {t('duplicates.keepThis')}
                </Button>
              </div>
            ))}
          </div>
        ))}
      </div>
    </div>
  )
}
