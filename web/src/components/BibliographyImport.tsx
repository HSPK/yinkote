/** Reading a `.bib` or `.ris` file into the library.
 *
 *  How most references actually arrive: a publisher's "download citation"
 *  button hands over one of these two, and without this they get retyped.
 *
 *  Unlike the Zotero import there is no preview step. That one merges a whole
 *  library into another and cannot be undone; this one adds a handful of
 *  records that can be selected and deleted, so a confirmation would be
 *  ceremony rather than protection.
 */
import { useRef, useState } from 'react'

import { api } from '../api/client'
import { useT } from '../i18n'
import { useStore } from '../state/store'
import { Button, toast } from '../ui'

export function BibliographyImport() {
  const t = useT()
  const library = useStore((s) => s.library)
  const refresh = useStore((s) => s.refresh)
  const reloadSidebar = useStore((s) => s.reloadSidebar)

  const picker = useRef<HTMLInputElement>(null)
  const [busy, setBusy] = useState(false)
  const [report, setReport] = useState<string | null>(null)

  const take = async (file: File) => {
    setBusy(true)
    setReport(null)
    try {
      const done = await api.import.bibliography(library, await file.text())
      setReport(
        done.skipped > 0
          ? t('import.bibSome', { imported: done.imported, skipped: done.skipped })
          : t('import.bibAll', { imported: done.imported }),
      )
      if (done.imported > 0) {
        await refresh()
        await reloadSidebar()
      }
    } catch (e) {
      toast.fromError(t('import.failed'), e)
    } finally {
      setBusy(false)
      // Chosen the same file twice in a row is a real thing to want, and the
      // input will not fire `change` again unless its value is cleared.
      if (picker.current) picker.current.value = ''
    }
  }

  return (
    <div className="import-row">
      <input
        ref={picker}
        type="file"
        accept=".bib,.ris,.txt,text/plain"
        className="hidden-input"
        onChange={(e) => {
          const file = e.target.files?.[0]
          if (file) void take(file)
        }}
      />
      <Button disabled={busy} onClick={() => picker.current?.click()}>
        {busy ? t('import.bibReading') : t('import.bibChoose')}
      </Button>
      <span className="dim">{report ?? t('import.bibHint')}</span>
    </div>
  )
}
