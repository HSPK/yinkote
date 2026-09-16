import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'

import { api } from '../api/client'
import type { Item } from '../api/types'
import { useT } from '../i18n'
import {
  HIGHLIGHT_COLOURS,
  inReadingOrder,
  toAnnotation,
  toDraft,
  type Annotation,
  type HighlightColour,
  type Mark,
} from '../lib/annotations'
import { useStore } from '../state/store'
import { Button, Empty, Icon, Splitter, contextMenu, toast, withToast } from '../ui'
import { PdfPage } from './PdfPage'
import { Outline } from '../components/Outline'
import { SelectionPopup } from '../components/SelectionPopup'
import { PageRail } from '../components/PageRail'
import { NoteCard } from '../components/NoteCard'
import { PdfSelectionOverlay } from '../components/PdfSelectionOverlay'
import { NoteView } from './NoteView'
import { AnnotationTools, type AnnotationPatch } from '../components/AnnotationTools'
import { loadOutline, type OutlineNode } from '../lib/outline'
import { useFind } from './useFind'
import { usePdf } from './usePdf'
import { copyText } from '../lib/clipboard'
import { displayTitle } from '../lib/format'
import { failureOf, type Failure } from '../lib/errors'
import { pdfSelection, type PageSelection } from '../lib/pdf-selection'
import { useReaderZoom, MIN_ZOOM, MAX_ZOOM } from './useReaderZoom'
import type { ScreenRect } from '../lib/selection-toolbar'

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

export function ReaderView({ target, active = true }: { target?: string; active?: boolean }) {
  const t = useT()
  // The tab owns its library even while another library is being browsed.
  const [library] = useState(() => useStore.getState().library)
  const [loaded, setLoaded] = useState<{ paper: string; attachments: Item[] } | null>(null)
  const [current, setCurrent] = useState<string | null>(null)
  const [problem, setProblem] = useState<Failure | null>(null)

  useEffect(() => {
    let live = true
    setLoaded(null)
    setProblem(null)
    setCurrent(null)
    if (!target) return
    void (async () => {
      const item = await api.items.get(library, target)
      const paper = item.itemType === 'attachment' ? item.parentKey ?? item.key : item.key
      const children = item.itemType === 'attachment' && !item.parentKey
        ? [item] : await api.items.children(library, paper)
      const attachments = children.filter((child) =>
        child.itemType === 'attachment' && (
          String(child.contentType ?? '').toLowerCase().split(';')[0] === 'application/pdf' ||
          (!child.contentType && /\.pdf$/i.test(String(child.filename ?? child.title ?? '')))
        ),
      )
      if (!live) return
      setLoaded({ paper, attachments })
      setCurrent(attachments.find((file) => file.key === target)?.key ?? attachments[0]?.key ?? null)
    })().catch((error: unknown) => live && setProblem(failureOf(error)))
    return () => { live = false }
  }, [library, target])

  if (!target) return <Empty>{t('reader.none')}</Empty>
  if (problem) return <Empty title={problem.detail}>{t('reader.unsupported')}</Empty>
  if (!loaded) return <Empty>{t('reader.loading')}</Empty>
  if (!current) return <Empty>{t('reader.noFile')}</Empty>
  return (
    <PdfReader
      key={`${library}:${current}`}
      target={loaded.paper}
      current={current}
      attachments={loaded.attachments}
      onFile={setCurrent}
      library={library}
      active={active}
    />
  )
}

function PdfReader({ target, current, attachments, onFile, library, active }: {
  target: string
  current: string
  attachments: Item[]
  onFile: (key: string) => void
  library: number
  active: boolean
}) {
  const t = useT()
  const layout = useStore((s) => s.readerLayout)
  const setLayout = useStore((s) => s.setReaderLayout)
  const [annotations, setAnnotations] = useState<Annotation[]>([])
  const [colour, setColour] = useState<HighlightColour>('amber')
  /** Which page is being read. Set where it is already worked out for saving,
   *  so there is one definition of it rather than two that can disagree. */
  const [page, setPage] = useState(1)
  const [pageInput, setPageInput] = useState('1')
  const citationStyle = useStore((s) => s.citationStyle)
  const [outline, setOutline] = useState<OutlineNode[]>([])
  /** Which of the paper's readings the side pane is showing. */
  const [notePane, setNotePane] = useState<'marks' | 'notes'>('marks')
  const [notes, setNotes] = useState<Item[]>([])
  const [openedNote, setOpenedNote] = useState<{ key: string; preview: boolean } | null>(null)
  const [chosenMark, setChosenMark] = useState<{
    key: string
    boxes: ScreenRect[]
    at: { x: number; y: number }
  } | null>(null)
  /** Pages near enough the viewport to draw. See the observer below. */
  const [near, setNear] = useState<Set<number>>(() => new Set([1, 2, 3]))
  /** Page one's size, which is every page's size in all but a handful of
   *  documents, and the only honest thing to reserve before measuring. */
  const [pageSize, setPageSize] = useState({ width: 0, height: 0 })
  /** A selection waiting for the reader to say what it is for. */
  const [pending, setPending] = useState<
    { selections: PageSelection[]; at: { x: number; y: number }; boxes: ScreenRect[] } | null
  >(null)
  const [selectionPages, setSelectionPages] = useState<Set<number>>(() => new Set())
  const [marking, setMarking] = useState(false)
  const [railTab, setRailTab] = useState<'pages' | 'outline'>('pages')
  const scrollRef = useRef<HTMLDivElement>(null)
  const savedScroll = useRef({ top: 0, left: 0 })
  const { zoom, renderZoom, setZoom, zoomAt, step } = useReaderZoom(scrollRef, active ? current : null)
  const reserve = useMemo(() => ({
    width: pageSize.width * zoom,
    height: pageSize.height * zoom,
  }), [pageSize, zoom])
  const report = useCallback((error: unknown) => useStore.setState({ error: failureOf(error) }), [])
  const live = useRef(true)
  useEffect(() => {
    live.current = true
    return () => { live.current = false }
  }, [])

  useEffect(() => {
    if (!active) {
      setPending(null)
      setChosenMark(null)
      setSelectionPages(new Set())
    }
  }, [active])

  useLayoutEffect(() => {
    const root = scrollRef.current
    if (!active || !root) return
    root.scrollTop = savedScroll.current.top
    root.scrollLeft = savedScroll.current.left
  }, [active])

  useEffect(() => {
    if (!active) return
    const retainSelection = () => {
      const root = scrollRef.current
      const selection = window.getSelection()
      const pageOf = (node: Node | null) => {
        const element = node instanceof Element ? node : node?.parentElement
        const page = element?.closest<HTMLElement>('.pdf-page')
        return page && root?.contains(page) ? Number(page.dataset.page) : null
      }
      const start = pageOf(selection?.anchorNode ?? null)
      const end = pageOf(selection?.focusNode ?? null)
      const pages = new Set<number>()
      if (selection && !selection.isCollapsed && start !== null && end !== null) {
        for (let page = Math.min(start, end); page <= Math.max(start, end); page += 1) pages.add(page)
      } else {
        setPending(null)
      }
      setSelectionPages((old) =>
        old.size === pages.size && [...old].every((page) => pages.has(page)) ? old : pages)
    }
    document.addEventListener('selectionchange', retainSelection)
    return () => document.removeEventListener('selectionchange', retainSelection)
  }, [active])

  const clearSelection = () => {
    const selection = window.getSelection()
    if (selection?.anchorNode && scrollRef.current?.contains(selection.anchorNode)) {
      selection.removeAllRanges()
    }
  }

  const file = api.files.url(library, current)
  const { doc, pages, error } = usePdf(file)

  // The toolbar's search box is find-in-document while a reader is in front.
  const globalFilter = useStore((s) => s.filter)
  const ownFilter = useRef(globalFilter)
  if (active) ownFilter.current = globalFilter
  const filter = ownFilter.current
  const find = useFind(doc, filter)
  const activeFind = useRef<string | null>(null)
  activeFind.current = find.active?.id ?? null
  const navigatedFind = useRef<string | null>(null)
  const showMatch = useCallback((id: string, rect: ScreenRect) => {
    const root = scrollRef.current
    if (!active || !root || activeFind.current !== id || navigatedFind.current === id) return
    navigatedFind.current = id
    const viewport = root.getBoundingClientRect()
    root.scrollTop += rect.top - viewport.top - root.clientHeight / 2 + rect.height / 2
    if (rect.left < viewport.left || rect.left + rect.width > viewport.right) {
      root.scrollLeft += rect.left - viewport.left - root.clientWidth / 2 + rect.width / 2
    }
  }, [active])

  useEffect(() => {
    if (!active) return
    const input = document.getElementById('search-input')
    if (!input) return
    const enter = (event: KeyboardEvent) => {
      if (event.key !== 'Enter') return
      event.preventDefault()
      find.go(event.shiftKey ? -1 : 1)
    }
    input.addEventListener('keydown', enter)
    return () => input.removeEventListener('keydown', enter)
  }, [active, find.go])

  const reload = useCallback(async () => {
    const kids = await api.items.children(library, current)
    if (live.current) setAnnotations(
      inReadingOrder(
        kids.filter((k) => k.itemType === 'annotation').flatMap((k) => toAnnotation(k) ?? []),
      ),
    )
  }, [library, current])

  useEffect(() => {
    void reload().catch(report)
  }, [reload, report])

  /** Turn whatever is selected into a highlight. */
  /** Remember what was selected and ask; write nothing yet. */
  const select = () => {
    const selection = window.getSelection()
    const root = scrollRef.current
    if (!selection || !root || marking) {
      setPending(null)
      return
    }
    const selections = pdfSelection(root, selection)
    if (!selections.length) {
      setPending(null)
      return
    }
    setChosenMark(null)
    const clip = root.getBoundingClientRect()
    const boxes = selections.flatMap((selected) => {
      const page = root.querySelector<HTMLElement>(`.pdf-page[data-page="${selected.page}"]`)
      if (!page) return []
      const box = page.getBoundingClientRect()
      return selected.rects.flatMap((rect) => {
        const left = Math.max(clip.left, box.left + rect.x * box.width)
        const top = Math.max(clip.top, box.top + rect.y * box.height)
        const right = Math.min(clip.right, box.left + (rect.x + rect.w) * box.width)
        const bottom = Math.min(clip.bottom, box.top + (rect.y + rect.h) * box.height)
        return right > left && bottom > top ? [{ left, top, width: right - left, height: bottom - top }] : []
      })
    })
    const last = boxes[boxes.length - 1]
    setPending({
      selections,
      boxes,
      at: { x: (last?.left ?? clip.left) + (last?.width ?? 0) / 2, y: last?.top ?? clip.top },
    })
  }

  const mark = async (kind: Mark, chosen: HighlightColour) => {
    if (!pending || marking) return
    setMarking(true)
    try {
      // A multi-page selection is one mark per page, with that page's own text
      // and coordinates. Never attach the whole selection to the release page.
      const drafts = pending.selections.map(({ page, rects, text }) =>
        toDraft(current, { page, rects }, text, chosen, kind),
      )
      const result = await api.items.create(library, drafts)
      if (!live.current) return
      const saved = new Set(result.created.map((item) => Number(item.annotationPage)))
      const remaining = pending.selections.filter((s) => !saved.has(s.page))
      setPending(remaining.length ? { ...pending, selections: remaining } : null)
      await reload()
      if (result.created.length !== drafts.length) {
        throw new Error(t('reader.partialHighlight'))
      }
      if (!live.current) return
      clearSelection()
      setColour(chosen)
      setPending(null)
    } catch (e) {
      toast.fromError(t('reader.highlightFailed'), e)
    } finally {
      if (live.current) setMarking(false)
    }
  }

  const copySelection = async () => {
    if (!pending) return
    try {
      await copyText(pending.selections.map((s) => s.text).join('\n'))
      clearSelection()
      setPending(null)
      toast.success(t('reader.copied'))
    } catch (error) {
      toast.fromError(t('reader.copyFailed'), error)
    }
  }

  /** The quoted sentence with a reference after it, which is what somebody
   *  reading a paper into their own notes actually wants. */
  const copyCitation = async () => {
    if (!pending || !target) return
    const quoted = `"${pending.selections.map((s) => s.text).join('\n')}"`
    try {
      const rendered = await api.citations.render(library, [target], citationStyle)
      const reference = rendered.citations[0] ?? ''
      await copyText(
        `${quoted} ${reference} ${t('reader.atPage', {
          page: pending.selections.map((s) => s.page).join(', '),
        })}`.trim(),
      )
      toast.success(t('reader.copied'))
    } catch (e) {
      toast.fromError(t('toast.citationFailed'), e)
    }
    clearSelection()
    setPending(null)
  }

  const remove = async (key: string) => {
    try {
      await api.items.destroy(library, [key])
      await reload()
    } catch (error) {
      toast.fromError(t('reader.removeFailed'), error)
    }
  }

  const editMark = (annotation: Annotation, boxes: ScreenRect[]) => {
    const root = scrollRef.current
    if (!root) return
    const clip = root.getBoundingClientRect()
    const visible = boxes.flatMap((box) => {
      const left = Math.max(clip.left, box.left)
      const top = Math.max(clip.top, box.top)
      const right = Math.min(clip.right, box.left + box.width)
      const bottom = Math.min(clip.bottom, box.top + box.height)
      return right > left && bottom > top ? [{ left, top, width: right - left, height: bottom - top }] : []
    })
    const first = visible[0]
    setPending(null)
    setChosenMark({
      key: annotation.key,
      boxes: visible,
      at: { x: first ? first.left + first.width / 2 : clip.left + 120, y: first?.top ?? clip.top },
    })
  }

  const updateMark = async (key: string, patch: AnnotationPatch) => {
    const original = annotations.find((annotation) => annotation.key === key)
    const updated = await api.items.update(library, key, { fields: patch }, original?.version)
    const annotation = toAnnotation(updated)
    if (!annotation) throw new Error(t('reader.annotationInvalid'))
    if (live.current) setAnnotations((all) => all.map((item) => item.key === key ? annotation : item))
  }

  const noteFromMark = async (key: string) => {
    const { note } = await api.noteFromAnnotations(library, target, [key])
    if (!live.current) return
    setNotes((all) => [note, ...all.filter((item) => item.key !== note.key)])
    setOpenedNote({ key: note.key, preview: false })
    setNotePane('notes')
    setLayout({ notesOpen: true })
    setChosenMark(null)
  }

  const selectedMark = chosenMark ? annotations.find((annotation) => annotation.key === chosenMark.key) : null

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
      const viewport = page.getViewport({ scale: 1 })
      setPageSize({ width: viewport.width, height: viewport.height })
    }).catch(report)
    return () => {
      live = false
    }
  }, [doc, report])

  /**
   * Draw only what is nearly on screen.
   *
   * One observer over all pages, with a viewport of overscan either way.
   *
   * Pages are forgotten once they leave, which is the point: a canvas at device
   * resolution is several megabytes, and a three-hundred-page thesis used to
   * render all three hundred before showing the first.
   */
  useEffect(() => {
    const root = scrollRef.current
    if (!active || !root || !doc) {
      setNear(new Set())
      return
    }
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
      { root, rootMargin: '100% 0px' },
    )
    root.querySelectorAll('[data-page]').forEach((el) => observer.observe(el))
    return () => observer.disconnect()
    // Observe once the boxes exist; zooming resizes those same boxes and does
    // not need to disconnect and re-observe an entire document.
  }, [active, doc, pages.length, Boolean(reserve.height)])

  /** Everything written about this paper: the summary, the close reading, and
   *  whatever the reader has typed. Loaded here so the pane can switch between
   *  them without a request per click. */
  useEffect(() => {
    if (!active) return
    if (!target) {
      setNotes([])
      return
    }
    let live = true
    void api.items
      .children(library, target)
      .then((kids) => {
        if (live) setNotes(kids.filter((k) => k.itemType === 'note'))
      })
      .catch(report)
    return () => {
      live = false
    }
  }, [library, target, layout.notesOpen, active, report])

  const byPage = useMemo(() => {
    const grouped = new Map<number, Annotation[]>()
    for (const annotation of annotations) {
      const list = grouped.get(annotation.page) ?? []
      list.push(annotation)
      grouped.set(annotation.page, list)
    }
    return grouped
  }, [annotations])

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

  const goTo = useCallback((requested: number) => {
    const root = scrollRef.current
    const page = Math.max(1, Math.min(pages.length, Math.round(requested)))
    const element = root?.querySelector<HTMLElement>(`.pdf-page[data-page="${page}"]`)
    if (!root || !element) return
    root.scrollTop += element.getBoundingClientRect().top - root.getBoundingClientRect().top - 12
    setPage(page)
    setPageInput(String(page))
  }, [pages.length])

  useEffect(() => {
    if (!find.active) {
      navigatedFind.current = null
      return
    }
    // Bring an unrendered match into view. Once its text geometry is ready,
    // showMatch positions the exact word; normal scrolling never resets it.
    if (active && reserve.height && navigatedFind.current !== find.active.id) goTo(find.active.pageNumber)
  }, [active, find.active?.id, Boolean(reserve.height), goTo])

  const [restored, setRestored] = useState(false)
  const resumePage = useRef<number | null>(null)
  useEffect(() => {
    let live = true
    void api.readerState
      .get(library, current)
      .then((state) => {
        if (!live) return
        setZoom(state.zoom)
        resumePage.current = state.lastPage
        setRestored(true)
      })
      .catch((error: unknown) => {
        if (!live) return
        report(error)
        setRestored(true)
      })
    return () => { live = false }
  }, [library, current, report, setZoom])

  useEffect(() => {
    if (!active || !restored || !doc || !reserve.height || resumePage.current === null) return
    goTo(resumePage.current)
    resumePage.current = null
  }, [active, restored, doc, reserve.height, goTo])

  // Update navigation once per frame and persist only after scrolling settles.
  // A tall page is still current while its top is offscreen.
  useEffect(() => {
    const scroller = scrollRef.current
    if (!active || !scroller || !doc || !restored || !reserve.height) return

    let timer = 0
    let frame = 0
    let position: { lastPage: number; zoom: number } | null = null
    const update = () => {
      frame = 0
      const top = scroller.getBoundingClientRect().top + 24
      let best = 1
      for (const el of scroller.querySelectorAll<HTMLElement>('.pdf-page')) {
        const box = el.getBoundingClientRect()
        if (box.top > top) break
        best = Number(el.dataset.page)
        if (box.bottom > top) break
      }
      setPage(best)
      setPageInput(String(best))
      position = { lastPage: best, zoom }
      window.clearTimeout(timer)
      timer = window.setTimeout(() => {
        timer = 0
        void api.readerState.put(library, current, { lastPage: best, zoom }).catch(report)
      }, SAVE_AFTER_MS)
    }
    const save = () => {
      setPending(null)
      setChosenMark(null)
      if (!frame) frame = requestAnimationFrame(update)
    }

    scroller.addEventListener('scroll', save, { passive: true })
    save()
    return () => {
      window.clearTimeout(timer)
      // Switching tabs or closing during the debounce must not lose the last
      // reading position. This closure still owns the original attachment.
      if (timer && position) void api.readerState.put(library, current, position).catch(report)
      cancelAnimationFrame(frame)
      scroller.removeEventListener('scroll', save)
    }
  }, [active, library, current, doc, zoom, restored, Boolean(reserve.height), report])

  const fitWidth = () => {
    const root = scrollRef.current
    if (!root || !reserve.width) return
    zoomAt((root.clientWidth - 24) / (reserve.width / zoom))
  }

  return (
    <div className="pane main reader" onKeyDown={(event) => {
      if (!event.ctrlKey && !event.metaKey) return
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) return
      if (event.key === '+' || event.key === '=') {
        event.preventDefault()
        step(1)
      } else if (event.key === '-') {
        event.preventDefault()
        step(-1)
      } else if (event.key === '0') {
        event.preventDefault()
        zoomAt(1)
      }
    }}>
      <div className="reader-bar">
        <button className="icon-btn" title={t('reader.navigation')} aria-label={t('reader.navigation')}
          aria-pressed={layout.navigation} onClick={() => setLayout({ navigation: !layout.navigation })}>
          <Icon.Panel size={13} />
        </button>
        {attachments.length > 1 && (
          <select className="reader-files" aria-label={t('reader.file')} value={current}
            onChange={(event) => onFile(event.target.value)}>
            {attachments.map((file) => <option key={file.key} value={file.key}>
              {displayTitle(file, String(file.filename ?? file.key))}
            </option>)}
          </select>
        )}
        <span className="reader-page-control">
          <button className="icon-btn" disabled={!doc || page <= 1} title={t('reader.previousPage')}
            onClick={() => goTo(page - 1)}><Icon.ChevronUp size={11} /></button>
          <input aria-label={t('reader.pageNumber')} inputMode="numeric" value={pageInput}
            onChange={(event) => setPageInput(event.target.value)}
            onBlur={() => {
              const number = Number(pageInput)
              if (Number.isFinite(number) && number > 0) goTo(number)
              else setPageInput(String(page))
            }}
            onKeyDown={(event) => {
              if (event.key === 'Enter') event.currentTarget.blur()
              if (event.key === 'Escape') setPageInput(String(page))
            }} />
          <span>/ {pages.length}</span>
          <button className="icon-btn" disabled={!doc || page >= pages.length} title={t('reader.nextPage')}
            onClick={() => goTo(page + 1)}><Icon.ChevronDown size={11} /></button>
        </span>

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
            {find.loading && <span className="dim" role="status">{t('reader.findIndexing')}</span>}
            {find.error && <span className="err" role="status" title={find.error.message}>
              {t('reader.findFailed')}
            </span>}
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
          disabled={zoom <= MIN_ZOOM}
          onClick={() => step(-1)}
        >
          <Icon.ChevronDown size={11} />
        </button>
        <button className="reader-zoom" title={t('reader.resetZoom')} onClick={() => zoomAt(1)}>
          {Math.round(zoom * 100)}%
        </button>
        <button
          className="icon-btn"
          title={t('reader.zoomIn')}
          disabled={zoom >= MAX_ZOOM}
          onClick={() => step(1)}
        >
          <Icon.ChevronUp size={11} />
        </button>
        <button className="chip" disabled={!doc} onClick={fitWidth}>{t('reader.fitWidth')}</button>
        <button className="icon-btn" title={t('reader.notesPanel')} aria-label={t('reader.notesPanel')}
          aria-pressed={layout.notesOpen} onClick={() => setLayout({ notesOpen: !layout.notesOpen })}>
          <Icon.Note size={13} />
        </button>
      </div>

      <div className="reader-body">
        {doc && layout.navigation && (
          <>
          <div className="reader-rail" style={{ width: layout.rail }}>
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
                doc={doc}
                pages={pages}
                current={page}
                onJump={goTo}
              />
            )}
          </div>
          <Splitter size={layout.rail} min={132} max={420} grows="left"
            onResize={(rail) => setLayout({ rail }, false)}
            onCommit={(rail) => setLayout({ rail })} />
          </>
        )}
        <div className="reader-pages" ref={scrollRef} tabIndex={0}
          aria-label={t('reader.document')}
          onScroll={(event) => {
            if (active) savedScroll.current = {
              top: event.currentTarget.scrollTop, left: event.currentTarget.scrollLeft,
            }
          }}
          onMouseDown={(event) => {
            if (marking) return
            setPending(null)
            setChosenMark(null)
            // Pin the anchor immediately, before autoscrolling can virtualise
            // it away. selectionchange extends the retained text range later.
            if (event.target instanceof Element) {
              const page = event.target.closest<HTMLElement>('.pdf-page')
              setSelectionPages(page ? new Set([Number(page.dataset.page)]) : new Set())
            }
          }}
          onMouseUp={select}
          onKeyUp={(event) => { if (event.shiftKey) select() }}>
          {error && <Empty title={error.detail}>{t('reader.unsupported')}</Empty>}
          {!doc && !error && <Empty>{t('reader.loading')}</Empty>}
          <PdfSelectionOverlay root={scrollRef} active={active} ready={doc} zoom={zoom} />
          {doc && reserve.height > 0 &&
            pages.map((n) => (
              <PdfPage
                key={n}
                doc={doc}
                pageNumber={n}
                zoom={zoom}
                renderZoom={renderZoom}
                annotations={byPage.get(n) ?? []}
                searchMatches={find.matchesByPage.get(n)}
                activeMatchId={find.active?.id}
                onMatchReady={showMatch}
                onAnnotationSelect={editMark}
                selectedAnnotation={chosenMark?.key}
                onRemove={remove}
                detail={!active ? 'none' : near.has(n) ? 'full'
                  : selectionPages.has(n) || find.active?.pageNumber === n ? 'text' : 'none'}
                reserve={reserve}
              />
            ))}
        </div>

        {layout.notesOpen && (
          <>
          <Splitter size={layout.notes} min={220} max={460} grows="right"
            onResize={(notes) => setLayout({ notes }, false)}
            onCommit={(notes) => setLayout({ notes })} />
        <aside className="reader-notes" style={{ width: layout.notes }}>
          {/* Everything written about this paper, reachable without leaving
              it. Reading is where a summary is wanted -- checking what the
              model said against the page in front of you -- and until now that
              meant going back to the library and opening another tab. */}
          <div className="rail-tabs">
            {(
              [
                ['marks', t('reader.marks')],
                ['notes', t('reader.notesTab', { count: notes.length })],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                className="rail-tab"
                data-active={notePane === id}
                onClick={() => setNotePane(id)}
              >
                {label}
              </button>
            ))}
          </div>

          {notePane === 'notes' && openedNote ? (
            <NoteView key={openedNote.key} target={openedNote.key} library={library} embedded initialPreview={openedNote.preview}
              onBack={() => setOpenedNote(null)}
              onSaved={(note) => setNotes((all) => all.map((item) => item.key === note.key ? note : item))} />
          ) : notePane === 'notes' && (
            <div className="reader-note-body note-list">
              {notes.length === 0 ? (
                  <Empty>{t('reader.noNotes')}</Empty>
                ) : (
                  notes.map((note) => <NoteCard key={note.key} note={note}
                    onOpen={() => setOpenedNote({ key: note.key, preview: true })} />)
              )}
            </div>
          )}

          {notePane === 'marks' && (
          <div className="reader-note-body note-list">
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
          </div>
          )}
        </aside>
          </>
        )}
      </div>

      {active && pending && !marking && (
        <SelectionPopup
          at={pending.at}
          selection={pending.boxes}
          colour={colour}
          onMark={mark}
          onCopy={copySelection}
          onCite={copyCitation}
          onDismiss={() => setPending(null)}
        />
      )}
      {active && chosenMark && selectedMark && (
        <AnnotationTools key={selectedMark.key} annotation={selectedMark}
          boxes={chosenMark.boxes} at={chosenMark.at}
          onChange={(patch) => updateMark(selectedMark.key, patch)}
          onNote={() => noteFromMark(selectedMark.key)}
          onDelete={async () => {
            await api.items.destroy(library, [selectedMark.key])
            if (live.current) {
              setAnnotations((all) => all.filter((item) => item.key !== selectedMark.key))
              setChosenMark(null)
            }
          }}
          onDismiss={() => setChosenMark(null)} />
      )}
    </div>
  )
}
