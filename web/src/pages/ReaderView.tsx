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
  type Mark,
} from '../lib/annotations'
import { useStore } from '../state/store'
import { Button, Empty, Icon, contextMenu, toast, withToast } from '../ui'
import { PdfPage } from './PdfPage'
import { Outline } from '../components/Outline'
import { SelectionPopup } from '../components/SelectionPopup'
import { PageRail } from '../components/PageRail'
import { loadOutline, type OutlineNode } from '../lib/outline'
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
  /** Which page is being read. Set where it is already worked out for saving,
   *  so there is one definition of it rather than two that can disagree. */
  const [page, setPage] = useState(1)
  const citationStyle = useStore((s) => s.citationStyle)
  const [outline, setOutline] = useState<OutlineNode[]>([])
  /** Pages near enough the viewport to draw. See the observer below. */
  const [near, setNear] = useState<Set<number>>(() => new Set([1, 2, 3]))
  /** Page one's size, which is every page's size in all but a handful of
   *  documents, and the only honest thing to reserve before measuring. */
  const [reserve, setReserve] = useState({ width: 0, height: 0 })
  /** A selection waiting for the reader to say what it is for. */
  const [pending, setPending] = useState<
    { page: number; rects: ReturnType<typeof rectsFromSelection>; text: string; at: { x: number; y: number } } | null
  >(null)
  const [railTab, setRailTab] = useState<'pages' | 'outline'>('pages')
  const scrollRef = useRef<HTMLDivElement>(null)

  const file = current ? api.files.url(library, current) : null
  const { doc, pages, error } = usePdf(file)

  // The toolbar's search box is find-in-document while a reader is in front.
  const filter = useStore((s) => s.filter)
  // The third argument is what tells find to look again. Text layers arrive as
  // pages are built, so the count of built pages belongs in it: a search run
  // while the document was still coming together would otherwise report only
  // what happened to exist at that moment.
  const find = useFind(
    scrollRef,
    filter,
    `${doc?.fingerprints?.[0] ?? ''}:${zoom}:${near.size}`,
  )

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
  /** Remember what was selected and ask; write nothing yet. */
  const select = (pageNumber: number, pageBox: DOMRect) => {
    const selection = window.getSelection()
    const text = selection?.toString().trim() ?? ''
    if (!selection || !text || !current) {
      setPending(null)
      return
    }
    const rects = rectsFromSelection(selection, pageBox)
    if (!rects.length) {
      setPending(null)
      return
    }
    // Anchored above the last rectangle, which is where the reader's eye and
    // pointer already are.
    const last = selection.getRangeAt(selection.rangeCount - 1).getClientRects()
    const box = last[last.length - 1]
    setPending({
      page: pageNumber,
      rects,
      text,
      at: { x: (box?.left ?? 0) + (box?.width ?? 0) / 2, y: (box?.top ?? 0) - 8 },
    })
  }

  const mark = async (kind: Mark, chosen: HighlightColour) => {
    if (!pending || !current) return
    window.getSelection()?.removeAllRanges()
    const { page: on, rects, text } = pending
    setPending(null)
    try {
      await api.items.create(library, [toDraft(current, { page: on, rects }, text, chosen, kind)])
      await reload()
    } catch (e) {
      toast.fromError(t('reader.highlightFailed'), e)
    }
  }

  const copyText = async () => {
    if (!pending) return
    await navigator.clipboard.writeText(pending.text).catch(() => {})
    window.getSelection()?.removeAllRanges()
    setPending(null)
    toast.success(t('reader.copied'))
  }

  /** The quoted sentence with a reference after it, which is what somebody
   *  reading a paper into their own notes actually wants. */
  const copyCitation = async () => {
    if (!pending || !target) return
    const quoted = `"${pending.text}"`
    try {
      const rendered = await api.citations.render(library, [target], citationStyle)
      const reference = rendered.citations[0] ?? ''
      await navigator.clipboard.writeText(
        `${quoted} ${reference} ${t('reader.atPage', { page: pending.page })}`.trim(),
      )
      toast.success(t('reader.copied'))
    } catch (e) {
      toast.fromError(t('toast.citationFailed'), e)
    }
    window.getSelection()?.removeAllRanges()
    setPending(null)
  }

  const remove = async (key: string) => {
    await api.items.destroy(library, [key])
    await reload()
  }

  /** How tall a page is, so an undrawn one can still hold its place.
   *
   *  Taken from page one at the current zoom. Without it every page would be
   *  zero-height until drawn, the scrollbar would be a lie, and scrolling to a
   *  page near the end would land somewhere else entirely.
   */
  useEffect(() => {
    if (!doc) return
    let live = true
    void doc.getPage(1).then((page) => {
      if (!live) return
      const viewport = page.getViewport({ scale: zoom })
      setReserve({ width: viewport.width, height: viewport.height })
    })
    return () => {
      live = false
    }
  }, [doc, zoom])

  /**
   * Draw only what is nearly on screen.
   *
   * One observer over all the pages rather than one per page, and a margin of
   * two viewports either way — the "±2 pages" the design asks for, expressed in
   * the units the browser actually measures in.
   *
   * Pages are forgotten once they leave, which is the point: a canvas at device
   * resolution is several megabytes, and a three-hundred-page thesis used to
   * render all three hundred before showing the first.
   */
  useEffect(() => {
    const root = scrollRef.current
    if (!root || !doc) return
    const observer = new IntersectionObserver(
      (entries) => {
        setNear((was) => {
          const now = new Set(was)
          for (const entry of entries) {
            const page = Number((entry.target as HTMLElement).dataset.page)
            if (entry.isIntersecting) now.add(page)
            else now.delete(page)
          }
          return now
        })
      },
      { root, rootMargin: '200% 0px' },
    )
    root.querySelectorAll('[data-page]').forEach((el) => observer.observe(el))
    return () => observer.disconnect()
    // `reserve.height` is in here because the pages do not exist until it is
    // known — observing before they are in the DOM observes nothing, silently.
  }, [doc, pages.length, reserve.height])

  /** The document's own table of contents, when it has one. */
  useEffect(() => {
    if (!doc) {
      setOutline([])
      return
    }
    let live = true
    void loadOutline(doc).then((nodes) => {
      if (!live) return
      setOutline(nodes)
      // Shown first when there is one: a reader who opened a thesis wants its
      // contents, not two hundred thumbnails.
      setRailTab(nodes.length ? 'outline' : 'pages')
    })
    return () => {
      live = false
    }
  }, [doc])

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
        setPage(best)
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
        {doc && current && pages.length > 1 && (
          <div className="reader-rail">
            {/* Only offered when the document has an outline. A tab that is
                always there and usually empty teaches people to ignore it. */}
            {outline.length > 0 && (
              <div className="rail-tabs">
                {(['outline', 'pages'] as const).map((tab) => (
                  <button
                    key={tab}
                    className="rail-tab"
                    data-active={railTab === tab}
                    onClick={() => setRailTab(tab)}
                  >
                    {t(tab === 'outline' ? 'reader.outline' : 'reader.pages')}
                  </button>
                ))}
              </div>
            )}
            {railTab === 'outline' && outline.length > 0 ? (
              <Outline nodes={outline} current={page} onJump={goTo} />
            ) : (
              <PageRail
                library={library}
                attachmentKey={current}
                pages={pages}
                current={page}
                onJump={goTo}
              />
            )}
          </div>
        )}
        <div className="reader-pages" ref={scrollRef} onMouseDown={() => setPending(null)}>
          {error && <Empty title={error.detail}>{t('reader.unsupported')}</Empty>}
          {!doc && !error && <Empty>{t('reader.loading')}</Empty>}
          {doc && reserve.height > 0 &&
            pages.map((n) => (
              <PdfPage
                key={n}
                doc={doc}
                pageNumber={n}
                zoom={zoom}
                annotations={annotations.filter((a) => a.page === n)}
                onSelect={select}
                onRemove={remove}
                // Searching asks about the whole document, and find works
                // over rendered spans — so a search brings every text layer
                // into being, without the canvases that make virtualising
                // worth doing.
                detail={near.has(n) ? 'full' : filter.trim() ? 'text' : 'none'}
                reserve={reserve}
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

      {pending && (
        <SelectionPopup
          at={pending.at}
          colour={colour}
          onMark={mark}
          onCopy={copyText}
          onCite={copyCitation}
          onDismiss={() => setPending(null)}
        />
      )}
    </div>
  )
}
