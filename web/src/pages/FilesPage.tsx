/** Every file in the library.
 *
 *  The storage directory is something people open in a file manager, sync
 *  between machines and grep. A view of it belongs here for the same reason the
 *  collection browser does: it is a thing you look *through*.
 *
 *  Renaming is offered in two halves — see what it would do, then do it.
 *  A batch rename nobody can look at first is one nobody should run.
 */
import { useCallback, useEffect, useState } from 'react'

import { api } from '../api/client'
import type { LibraryFile, RenamePlan } from '../api/types'
import { useT } from '../i18n'
import { bytes as formatBytes } from '../lib/format'
import { useStore } from '../state/store'
import { Button, Empty, Icon, Input, toast } from '../ui'
import { VirtualList } from '../components/VirtualList'

/** Narrower than this the columns scroll sideways rather than crush. */
const FILE_COLUMNS = 720

export function FilesPage() {
  const t = useT()
  const library = useStore((s) => s.library)
  const showItem = useStore((s) => s.showItem)
  const setFileCount = useStore((s) => s.setFileCount)

  const [files, setFiles] = useState<LibraryFile[]>([])
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(true)
  const [template, setTemplate] = useState('')
  const [plan, setPlan] = useState<RenamePlan | null>(null)
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    const page = await api.files.list(library).catch(() => null)
    if (page) {
      setFiles(page.files)
      setTotal(page.total)
      setFileCount(page.total)
    }
    setLoading(false)
  }, [library, setFileCount])

  useEffect(() => {
    void load()
  }, [load])

  const look = async () => {
    setBusy(true)
    try {
      const next = await api.files.preview(library, template)
      setPlan(next)
      // The template the server settled on, so the box shows what was used
      // rather than what was typed — they differ when the box was left empty.
      setTemplate(next.template)
    } catch (e) {
      toast.fromError(t('files.renameFailed'), e)
    } finally {
      setBusy(false)
    }
  }

  const apply = async () => {
    setBusy(true)
    try {
      const done = await api.files.rename(library, template)
      toast.success(t('files.renamed', { count: done.renamed }))
      setPlan(null)
      await load()
    } catch (e) {
      toast.fromError(t('files.renameFailed'), e)
    } finally {
      setBusy(false)
    }
  }

  if (loading) return <Empty>{t('files.loading')}</Empty>

  return (
    <div className="pane main data-page">
      <div className="gaps-bar">
        <span className="dim">{t('files.summary', { count: total })}</span>
        <span className="row-actions">
          <Input
            value={template}
            spellCheck={false}
            placeholder={t('files.templatePlaceholder')}
            onChange={(e) => setTemplate(e.target.value)}
          />
          <Button disabled={busy} onClick={() => void look()}>
            {t('files.preview')}
          </Button>
          {/* The label says what is happening, not just that the button is
              off. A greyed-out button with its usual name explains nothing —
              renaming every file in a library is not instant. */}
          <Button tone="primary" disabled={busy || !plan?.total} onClick={() => void apply()}>
            {busy ? t('files.renaming') : t('files.rename')}
          </Button>
        </span>
      </div>

      {plan && (
        <div className="rename-plan">
          {plan.total === 0 ? (
            <span className="dim">{t('files.nothingToRename')}</span>
          ) : (
            <>
              <div className="dim">{t('files.willRename', { count: plan.total })}</div>
              {plan.changes.slice(0, 8).map((change) => (
                <div key={change.key} className="rename-row">
                  <span className="dim">{change.from}</span>
                  <Icon.ChevronDown size={10} />
                  <span>{change.to}</span>
                </div>
              ))}
            </>
          )}
        </div>
      )}

      <VirtualList
        rows={files}
        keyOf={(file) => file.key}
        minWidth={FILE_COLUMNS}
        header={
          <div className="table-head files-grid">
            <div className="head-cell">{t('files.col.name')}</div>
            <div className="head-cell">{t('files.col.paper')}</div>
            <div className="head-cell">{t('files.col.source')}</div>
            <div className="head-cell num">{t('files.col.size')}</div>
          </div>
        }
        empty={<Empty>{t('files.none')}</Empty>}
      >
        {(file) => (
          <div
            className="row browser-grid files-grid"
            onClick={() => file.parentKey && void showItem(file.parentKey)}
          >
            <div className="cell name-cell" title={file.filename}>
              <Icon.Library className="glyph" />
              <span className="name">{file.filename}</span>
            </div>
            <div className="cell dim" title={file.parentTitle}>
              {file.parentTitle}
            </div>
            {/* Where it came from: the question a file browser is opened to
                answer, and the reason the address is kept on the attachment. */}
            <div className="cell dim mono" title={file.url}>
              {file.url}
            </div>
            <div className="cell num dim">{formatBytes(file.bytes)}</div>
          </div>
        )}
      </VirtualList>
    </div>
  )
}
