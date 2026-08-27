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
import { Button, Empty, Icon, contextMenu, toast, withToast } from '../ui'
import { PdfPage } from './PdfPage'
import { useFind } from './useFind'
import { usePdf } from './usePdf'

/**
 * Reads and annotates an item's PDF.
 *
 * A tab, not a modal: reading is not a detour from the library, it is what the
 * library is for, and it must survive searching, chatting and note-taking.
 */
/** How long the reader must sit still before its place is written down.
 *
 *  Scrolling fires continuously; one request per event would be hundreds a
 *  minute for something nobody is waiting on. */
const SAVE_AFTER_MS = 600

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

  const goTo = useCallback((page: number, smooth = true) => {
    scrollRef.current
      ?.querySelector(`[data-page="${page}"]`)
      ?.scrollIntoView({ block: 'start', behavior: smooth ? 'smooth' : 'auto' })
  }, [])

  /** Reopen where it was left.
   *
   *  Waits for the pages to exist: the state arrives long before the PDF is
   *  rendered, and scrolling to a page that is not in the document yet does
   *  nothing at all — silently, which is the sort of bug that gets called
   *  "sometimes it works".
   */
  useEffect(() => {
    if (!current || !doc || !pages.length) return
    let live = true
    void api.readerState
      .get(library, current)
      .then((state) => {
        if (!live) return
        setZoom(state.zoom)
        // No animation: this is where the document opens, not somewhere it
        // travelled to.
        if (state.lastPage > 1) window.setTimeout(() => live && goTo(state.lastPage, false), 0)
      })
      .catch(() => {})
    return () => {
      live = false
    }
    // Deliberately not `zoom`: this restores it, and depending on it would
    // undo every zoom the reader makes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [library, current, doc, pages.length, goTo])

  /** Remember the page in view, without saying so on every scrolled pixel. */
  useEffect(() => {
    const scroller = scrollRef.current
    if (!scroller || !current || !doc) return

    let timer = 0
    const save = () => {
      window.clearTimeout(timer)
      timer = window.setTimeout(() => {
        const top = scroller.getBoundingClientRect().top
        // The page whose top edge is nearest the top of the view is the one
        // being read; the first one *fully* visible is wrong at any zoom where
        // a page is taller than the pane.
        let best = 1
        let bestGap = Number.POSITIVE_INFINITY
        for (const el of scroller.querySelectorAll('[data-page]')) {
          const gap = Math.abs(el.getBoundingClientRect().top - top)
          if (gap < bestGap) {
            bestGap = gap
            best = Number((el as HTMLElement).dataset.page ?? 1)
          }
        }
        void api.readerState.put(library, current, { lastPage: best, zoom }).catch(() => {})
      }, SAVE_AFTER_MS)
    }

    scroller.addEventListener('scroll', save, { passive: true })
    // Zoom is a decision rather than a drift, so it is worth saving on its own.
    save()
    return () => {
      window.clearTimeout(timer)
      scroller.removeEventListener('scroll', save)
    }
  }, [library, current, doc, zoom])

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
          <div className="pane-header">
            <span>{t('reader.annotations', { count: annotations.length })}</span>
            {/* Where the highlights are is where somebody decides they are
                finished with them, so the action to keep them lives here. */}
            {annotations.length > 0 && target && (
              <Button
                tone="ghost"
                title={t('reader.gatherHint')}
                onClick={() =>
                  void withToast(
                    async () => await api.noteFromAnnotations(library, target),
                    {
                      success: (made) =>
                        t('reader.gathered', { count: made?.annotations ?? 0 }),
                      failure: t('reader.gatherFailed'),
                    },
                  )
                }
              >
                {t('reader.gather')}
              </Button>
            )}
          </div>
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
              {/* A margin note highlights nothing, so its comment *is* its
                  text. Showing only the quoted passage rendered every imported
                  Zotero note as an empty card. */}
              {a.text && <span className="note-text">{a.text}</span>}
              {a.comment && <span className="note-comment">{a.comment}</span>}
              {!a.text && !a.comment && <span className="note-text dim">{t('reader.blankNote')}</span>}
            </button>
          ))}
        </aside>
      </div>
    </div>
  )
}
