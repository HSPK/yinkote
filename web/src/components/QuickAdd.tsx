import { useRef, useState } from 'react'

import { api } from '../api/client'
import { useStore } from '../state/store'
import { toast } from '../ui'
import { useT } from '../i18n'

/** Roughly what the server will recognise, so the hint can appear instantly
 *  without a round trip. Detection proper still happens server-side. */
function guessKind(text: string): string | null {
  const t = text.trim()
  if (!t) return null
  if (/\b10\.\d{4,9}\//.test(t)) return 'DOI'
  if (/arxiv[.:/]/i.test(t) || /^\d{4}\.\d{4,5}(v\d+)?$/.test(t)) return 'arXiv'
  if (/\bpmid\b/i.test(t) || /pubmed\.ncbi/i.test(t)) return 'PubMed'
  if (/\bisbn\b/i.test(t) || /^[\d-]{10,17}$/.test(t)) return 'ISBN'
  if (/^https?:\/\//i.test(t)) return 'URL'
  return null
}

/**
 * Paste a DOI, arXiv link, ISBN or publisher URL and press Enter.
 *
 * Adds immediately rather than opening a confirmation step — the toast carries
 * an Undo, which is faster than a modal for the common case where it is right.
 */
export function QuickAdd() {
  const t = useT()
  const library = useStore((s) => s.library)
  const collection = useStore((s) => s.collection)
  const view = useStore((s) => s.view)
  const refresh = useStore((s) => s.refresh)
  const reloadSidebar = useStore((s) => s.reloadSidebar)

  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)

  const kind = guessKind(text)

  // Takes an override because `onPaste` submits before React has re-rendered
  // with the new value.
  const submit = async (override?: string) => {
    const input = (override ?? text).trim()
    if (!input || busy) return
    setBusy(true)
    try {
      const result = await api.scrape.quickAdd(library, {
        text: input,
        collection: view === 'collection' && collection ? collection : undefined,
      })

      if (result.created.length) {
        const first = result.created[0]!
        const title = String(first.title ?? t('detail.untitled'))
        toast.success(
          result.created.length > 1
            ? t('quickAdd.addedMore', { title, count: result.created.length })
            : t('quickAdd.added', { title }),
        )
        useStore.setState({ selected: [first.key] })
        setText('')
      } else if (result.duplicates.length) {
        const dup = result.duplicates[0]!
        toast.info(t('quickAdd.duplicate'), dup.title)
        useStore.setState({ selected: [dup.existingKey] })
        setText('')
      } else {
        toast.error(t('quickAdd.noMetadata'))
      }

      await Promise.all([refresh(), reloadSidebar()])
    } catch (error) {
      toast.fromError(t('quickAdd.failed'), error)
    } finally {
      setBusy(false)
      inputRef.current?.focus()
    }
  }

  return (
    <div className="quick-add" data-busy={busy || undefined}>
      <input
        ref={inputRef}
        id="quick-add-input"
        value={text}
        disabled={busy}
        spellCheck={false}
        autoComplete="off"
        placeholder={t('quickAdd.placeholder')}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') void submit()
          if (e.key === 'Escape') {
            setText('')
            e.currentTarget.blur()
          }
        }}
        // Pasting is the dominant interaction; submit straight away.
        onPaste={(e) => {
          const pasted = e.clipboardData.getData('text').trim()
          if (!pasted || text.trim()) return
          e.preventDefault()
          setText(pasted)
          void submit(pasted)
        }}
      />
      {kind && !busy && <span className="quick-add-kind">{kind}</span>}
      {busy && <span className="quick-add-kind" data-busy="true">{t('quickAdd.resolving')}</span>}
    </div>
  )
}
