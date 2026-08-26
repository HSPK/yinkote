import { useCallback, useEffect, useRef, useState } from 'react'

import { api } from '../api/client'
import type { Item } from '../api/types'
import { useT } from '../i18n'
import {
  HIGHLIGHT_COLOURS,
  inReadingOrder,
  rectsFromSelection,
  toAnnotation,
  toDraft,
  type Annotation,
  type HighlightColour,
} from '../lib/annotations'
import { useStore } from '../state/store'
import { Empty, Icon, contextMenu, toast } from '../ui'
import { PdfPage } from './PdfPage'
import { useFind } from './useFind'
import { usePdf } from './usePdf'

/**
 * Reads and annotates an item's PDF.
 *
 * A tab, not a modal: reading is not a detour from the library, it is what the
 * library is for, and it must survive searching, chatting and note-taking.
 */
export function ReaderView({ target }: { target?: string }) {
  const t = useT()
  const library = useStore((s) => s.library)

  const [attachments, setAttachments] = useState<Item[]>([])
  const [current, setCurrent] = useState<string | null>(null)
  const [annotations, setAnnotations] = useState<Annotation[]>([])
  const [colour, setColour] = useState<HighlightColour>('amber')
  const [zoom, setZoom] = useState(1.2)
  const scrollRef = useRef<HTMLDivElement>(null)

  const file = current ? api.files.url(library, current) : null
  const { doc, pages, error } = usePdf(file)

  // The toolbar's search box is find-in-document while a reader is in front.
  const filter = useStore((s) => s.filter)
  const find = useFind(scrollRef, filter, `${doc?.fingerprints?.[0] ?? ''}:${zoom}`)

  useEffect(() => {
    if (!target) return
    let live = true
    api.items
      .children(library, target)
      .then((kids) => {
        if (!live) return
        const files = kids.filter((k) => k.itemType === 'attachment')
        setAttachments(files)
        setCurrent((c) => c ?? files[0]?.key ?? null)
      })
      .catch(() => setAttachments([]))
    return () => {
      live = false
    }
  }, [library, target])

  const reload = useCallback(async () => {
    if (!current) return setAnnotations([])
    const kids = await api.items.children(library, current).catch(() => [])
    setAnnotations(
      inReadingOrder(
        kids.filter((k) => k.itemType === 'annotation').flatMap((k) => toAnnotation(k) ?? []),
      ),
    )
  }, [library, current])

  useEffect(() => {
    void reload()
  }, [reload])

  /** Turn whatever is selected into a highlight. */
  const highlight = async (pageNumber: number, pageBox: DOMRect) => {
    const selection = window.getSelection()
    const text = selection?.toString().trim() ?? ''
    if (!selection || !text || !current) return

    const rects = rectsFromSelection(selection, pageBox)
    if (!rects.length) return

    selection.removeAllRanges()
    try {
      await api.items.create(library, [toDraft(current, { page: pageNumber, rects }, text, colour)])
      await reload()
    } catch (e) {
      toast.fromError(t('reader.highlightFailed'), e)
    }
  }

  const remove = async (key: string) => {
    await api.items.destroy(library, [key])
    await reload()
  }

  const goTo = (page: number) => {
    scrollRef.current
      ?.querySelector(`[data-page="${page}"]`)
      ?.scrollIntoView({ block: 'start', behavior: 'smooth' })
  }

  if (!target) return <Empty>{t('reader.none')}</Empty>
  if (!attachments.length) return <Empty>{t('reader.noFile')}</Empty>

  return (
    <div className="pane main reader">
      <div className="reader-bar">
        {attachments.length > 1 && (
          <div className="reader-files">
            {attachments.map((f) => (
              <button key={f.key} data-active={f.key === current} onClick={() => setCurrent(f.key)}>
                {String(f.title ?? f.filename ?? f.key)}
              </button>
            ))}
          </div>
        )}

        <div className="swatches" title={t('reader.colour')}>
          {HIGHLIGHT_COLOURS.map((c) => (
            <button
              key={c}
              className="swatch"
              data-colour={c}
              data-active={colour === c}
              onClick={() => setColour(c)}
            />
          ))}
        </div>

        <span className="spacer" />

        {filter && (
          <span className="find-nav">
            <span className="dim">
              {find.total ? t('search.matches', { index: find.index, total: find.total }) : t('search.noMatches')}
            </span>
            <button
              className="icon-btn"
              title={t('search.previous')}
              disabled={!find.total}
              onClick={() => find.go(-1)}
            >
              <Icon.ChevronUp size={11} />
            </button>
            <button
              className="icon-btn"
              title={t('search.next')}
              disabled={!find.total}
              onClick={() => find.go(1)}
            >
              <Icon.ChevronDown size={11} />
            </button>
          </span>
        )}

        <button
          className="icon-btn"
          title={t('reader.zoomOut')}
          onClick={() => setZoom((z) => Math.max(0.5, z - 0.2))}
        >
          <Icon.ChevronDown size={11} />
        </button>
        <span className="reader-zoom">{Math.round(zoom * 100)}%</span>
        <button
          className="icon-btn"
          title={t('reader.zoomIn')}
          onClick={() => setZoom((z) => Math.min(3, z + 0.2))}
        >
          <Icon.ChevronUp size={11} />
        </button>
      </div>

      <div className="reader-body">
        <div className="reader-pages" ref={scrollRef}>
          {error && <Empty>{t('reader.unsupported')}</Empty>}
          {!doc && !error && <Empty>{t('reader.loading')}</Empty>}
          {doc &&
            pages.map((n) => (
              <PdfPage
                key={n}
                doc={doc}
                pageNumber={n}
                zoom={zoom}
                annotations={annotations.filter((a) => a.page === n)}
                onHighlight={highlight}
                onRemove={remove}
              />
            ))}
        </div>

        <aside className="reader-notes">
          <div className="pane-header">{t('reader.annotations', { count: annotations.length })}</div>
          {annotations.length === 0 && <Empty>{t('reader.noAnnotations')}</Empty>}
          {annotations.map((a) => (
            <button
              key={a.key}
              className="note-card"
              data-colour={a.colour}
              onClick={() => goTo(a.page)}
              onContextMenu={contextMenu(() => [
                { label: t('menu.delete'), danger: true, onSelect: () => void remove(a.key) },
              ])}
            >
              <span className="note-page">{t('reader.page', { page: a.page })}</span>
              <span className="note-text">{a.text}</span>
            </button>
          ))}
        </aside>
      </div>
    </div>
  )
}
