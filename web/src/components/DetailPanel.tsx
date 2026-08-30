import { useCallback, useEffect, useState } from 'react'

import { api } from '../api/client'
import type { BadgeValue, Citation, CitationList, Conversation, Item } from '../api/types'
import { creatorName, displayTitle } from '../lib/format'
import { tagColour } from '../lib/tags'
import { useStore } from '../state/store'
import { useSchemaLabel, useT } from '../i18n'
import { useDebounced } from '../lib/useDebounced'
import { Badge, Icon, toast } from '../ui'
import { Thumbnail } from './Thumbnail'

/** An edit in progress, and which paper it belongs to. */
export interface Edit {
  key: string
  value: string
}

/** What the box shows: the edit only when it belongs to the paper on screen.
 *
 *  Between selecting another paper and the re-render settling, one editor
 *  instance holds the previous paper's text and the new paper's key. Deriving
 *  what to show — rather than keeping a copy in step with an effect — means
 *  that window shows the new paper's stored value instead of the old paper's
 *  edit. */
export function shownValue(edit: Edit, itemKey: string, stored: string): string {
  return edit.key === itemKey ? edit.value : stored
}

/** Whether this edit should be written to this paper.
 *
 *  Two conditions, and the first is the one that was missing: an edit made on
 *  a paper that is no longer shown belongs to nothing, and saving it wrote one
 *  paper's publication onto another with no way to undo it. */
export function worthSaving(edit: Edit, itemKey: string, stored: string): boolean {
  return edit.key === itemKey && edit.value !== stored
}

/** Fields worth a multi-line editor. */
const LONG_FIELDS = new Set(['abstractNote', 'extra', 'note'])

function FieldEditor({ item, field, label }: { item: Item; field: string; label: string }) {
  const patchItem = useStore((s) => s.patchItem)
  const stored = String(item[field] ?? '')

  // An edit in progress, and *whose* it is.
  //
  // This used to be a bare `value` kept in step with the prop by an effect,
  // and that lost a paper's publication to whichever paper was clicked next:
  // one component instance serves every selection, so on switching from A to
  // B it re-rendered holding A's text and B's key, and the blur that followed
  // wrote A's publication onto B. Silently, and with nothing to undo it.
  //
  // Carrying the owner makes that unrepresentable rather than merely unlikely:
  // an edit belonging to a paper that is no longer shown is neither displayed
  // nor saved. State mirroring props was the bug; this derives instead.
  const [edit, setEdit] = useState<Edit>({ key: item.key, value: stored })
  const value = shownValue(edit, item.key, stored)
  const setValue = (next: string) => setEdit({ key: item.key, value: next })

  const commit = () => {
    if (!worthSaving(edit, item.key, stored)) return
    void patchItem(item.key, { fields: { [field]: value === '' ? null : value } })
  }

  return (
    <>
      <dt title={field}>{label}</dt>
      <dd>
        {LONG_FIELDS.has(field) ? (
          <textarea
            rows={field === 'abstractNote' ? 5 : 2}
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onBlur={commit}
          />
        ) : (
          <input
            value={value}
            spellCheck={false}
            onChange={(e) => setValue(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === 'Enter') e.currentTarget.blur()
              if (e.key === 'Escape') {
                setValue(stored)
                e.currentTarget.blur()
              }
            }}
          />
        )}
      </dd>
    </>
  )
}

function TagEditor({ item }: { item: Item }) {
  const t = useT()
  const patchItem = useStore((s) => s.patchItem)
  // The stored colours live with the tag list, not on the item's tags — an
  // item carries names, and a name is what a colour belongs to.
  const tagColours = useStore((s) => s.tagColours)
  const [draft, setDraft] = useState('')

  const setTags = (tags: { tag: string; type?: number }[]) =>
    void patchItem(item.key, { tags })

  return (
    <>
      <dt>{t('detail.tags')}</dt>
      <dd>
        <div className="chip-row">
          {item.tags.map((tag) => (
            <span
              key={tag.tag}
              className="chip"
              data-colour={tagColour(tag.tag, tagColours[tag.tag])}
              title={tag.type === 1 ? t('detail.tagAuto') : t('detail.tagManual')}
            >
              {tag.tag}
              <button
                onClick={() => setTags(item.tags.filter((x) => x.tag !== tag.tag))}
                title={t('detail.remove')}
              >
                ×
              </button>
            </span>
          ))}
          <input
            value={draft}
            placeholder={t('detail.addTag')}
            spellCheck={false}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key !== 'Enter') return
              const name = draft.trim()
              if (name && !item.tags.some((t) => t.tag === name)) {
                setTags([...item.tags, { tag: name, type: 0 }])
              }
              setDraft('')
            }}
          />
        </div>
      </dd>
    </>
  )
}

export function DetailPanel() {
  const t = useT()
  const label = useSchemaLabel()
  const schema = useStore((s) => s.schema)
  const items = useStore((s) => s.items)
  const selected = useStore((s) => s.selected)
  const patchItem = useStore((s) => s.patchItem)
  const collections = useStore((s) => s.collections)
  const addSelectedToCollection = useStore((s) => s.addSelectedToCollection)

  const detached = useStore((s) => s.detached)

  // The list is the usual source, but not the only one: a graph neighbour is
  // shown here without ever appearing in the table behind it. The key check is
  // what makes a stale detached item harmless.
  const item =
    items.find((i) => i.key === selected[0]) ??
    (detached?.key === selected[0] ? detached : undefined)

  if (!item) {
    return (
      <aside className="pane">
        <div className="pane-header">{t('detail.title')}</div>
        <div className="empty">
          {selected.length > 1
            ? t('detail.multiple', { count: selected.length })
            : t('detail.none')}
        </div>
      </aside>
    )
  }

  const typeDef = schema?.itemTypes.find((t) => t.type === item.itemType)
  const fields = typeDef?.fields ?? ['title', 'date', 'abstractNote']

  return (
    <aside className="pane">
      <div className="pane-header">
        {t('detail.title')}
        <span className="spacer" />
        <span style={{ fontFamily: 'var(--mono)' }}>{item.key}</span>
        <span>v{item.version}</span>
      </div>

      <div className="detail">
        <div className="detail-title">{displayTitle(item, t('detail.untitled'))}</div>

        <dl className="field-grid">
          <dt>{t('detail.type')}</dt>
          <dd>
            <select
              value={item.itemType}
              onChange={(e) => void patchItem(item.key, { itemType: e.target.value })}
            >
              {schema?.itemTypes
                .filter((d) => !d.internal)
                .map((d) => (
                  <option key={d.type} value={d.type}>
                    {label(d, d.type)}
                  </option>
                ))}
            </select>
          </dd>

          <dt>{t('detail.authors')}</dt>
          <dd>
            <div className="chip-row">
              {item.creators.map((c, i) => (
                <span key={i} className="chip">
                  {creatorName(c)}
                  <button
                    title={t('detail.remove')}
                    onClick={() =>
                      void patchItem(item.key, {
                        creators: item.creators.filter((_, j) => j !== i),
                      })
                    }
                  >
                    ×
                  </button>
                </span>
              ))}
              <input
                placeholder={t('detail.addAuthor')}
                spellCheck={false}
                onKeyDown={(e) => {
                  if (e.key !== 'Enter') return
                  const raw = e.currentTarget.value.trim()
                  if (!raw) return
                  // "Wei Zhang" splits; a single token (or CJK) stays one field.
                  const parts = raw.split(/\s+/)
                  const creator =
                    parts.length > 1
                      ? { creatorType: 'author', firstName: parts.slice(0, -1).join(' '), lastName: parts.at(-1) }
                      : { creatorType: 'author', name: raw }
                  void patchItem(item.key, { creators: [...item.creators, creator] })
                  e.currentTarget.value = ''
                }}
              />
            </div>
          </dd>

          <TagEditor item={item} />

          {fields
            .filter((f) => f !== 'title')
            .map((f) => (
              <FieldEditor
                // Keyed by the paper as well as the field, so selecting
                // another one builds a fresh editor rather than handing the
                // previous paper's half-finished edit to it. The editor
                // guards this itself too; this makes it structural.
                key={`${item.key}:${f}`}
                item={item}
                field={f}
                label={label(schema?.fields[f], f)}
              />
            ))}

          <dt>{t('detail.collections')}</dt>
          <dd>
            <div className="chip-row">
              {item.collections.map((k) => (
                <span key={k} className="chip">
                  {collections.find((c) => c.key === k)?.name ?? k}
                </span>
              ))}
              <select
                value=""
                onChange={(e) => {
                  if (e.target.value) void addSelectedToCollection(e.target.value)
                }}
              >
                <option value="">{t('detail.addCollection')}</option>
                {collections
                  .filter((c) => !item.collections.includes(c.key))
                  .map((c) => (
                    <option key={c.key} value={c.key}>
                      {c.name}
                    </option>
                  ))}
              </select>
            </div>
          </dd>

          <ItemMetrics itemKey={item.key} />
          <ItemCover itemKey={item.key} />
          <ItemNotes itemKey={item.key} />
          <ItemReferences itemKey={item.key} />
          <ItemConversations itemKey={item.key} />
        </dl>
      </div>
    </aside>
  )
}

/** What has already been asked about this paper.
 *
 *  Asked from the paper rather than from the chat list: standing on something
 *  you are reading, "what did I already work out about this" is a question the
 *  library should answer without making you remember which thread it was in.
 */
function ItemConversations({ itemKey: selected }: { itemKey: string }) {
  const t = useT()
  const itemKey = useDebounced(selected)
  const library = useStore((s) => s.library)
  const openConversation = useStore((s) => s.openConversation)
  const askAbout = useStore((s) => s.askAbout)
  const [found, setFound] = useState<Conversation[]>([])

  useEffect(() => {
    let live = true
    void api.conversations
      .aboutItem(library, itemKey)
      .then((r) => {
        if (live) setFound(r.conversations)
      })
      .catch(() => {
        if (live) setFound([])
      })
    return () => {
      live = false
    }
  }, [library, itemKey])

  return (
    <>
      <dt>{t('item.conversations')}</dt>
      <dd>
        <div className="chip-row">
          {found.map((c) => (
            <button
              key={c.key}
              className="chip"
              onClick={() => void openConversation(c.key)}
              title={c.title}
            >
              {c.title || c.key}
            </button>
          ))}
          {!found.length && <span className="dim">{t('item.conversationsNone')}</span>}
          <button className="chip" onClick={() => void askAbout(itemKey)}>
            {t('item.askAboutThis')}
          </button>
        </div>
      </dd>
    </>
  )
}

/** What this paper stands on, and what stands on it.
 *
 *  The citation data has been stored and used by the graph since it arrived,
 *  with nowhere to read it plainly — so the most direct question anybody has
 *  of it ("what does this cite, and which of those do I have?") had no answer
 *  in the interface.
 *
 *  A cited work the library holds is a link; one it does not is the label the
 *  publisher printed. Neither case is special, which is the point.
 */
/** What the badge plugins say about this paper, and who said it.
 *
 *  The columns show a number in a few characters; this shows the sentence
 *  behind it. An impact factor with no stated source is one a reader cannot
 *  check — and when two papers here carried invented journal names, a
 *  perfectly correct number beside the wrong journal looked like a broken
 *  plugin. Naming the journal the metric was matched to makes that visible.
 */
/** What a plugin says about its own number. Not an item title, despite the
 *  field's name -- it is the sentence the plugin wrote to attribute it. */
function attribution({ title, badge }: BadgeValue): string {
  return title ?? badge
}

function ItemMetrics({ itemKey: selected }: { itemKey: string }) {
  const t = useT()
  const itemKey = useDebounced(selected)
  const library = useStore((s) => s.library)
  const [values, setValues] = useState<BadgeValue[]>([])

  useEffect(() => {
    let live = true
    void api.badges
      .resolve(library, [itemKey])
      .then((all) => {
        if (live) setValues(all[itemKey] ?? [])
      })
      .catch(() => {
        if (live) setValues([])
      })
    return () => {
      live = false
    }
  }, [library, itemKey])

  if (values.length === 0) return null

  return (
    <>
      <dt>{t('detail.metrics')}</dt>
      <dd>
        <div className="metric-list">
          {values.map((v) => (
            <div key={`${v.pluginId}:${v.badge}`} className="metric-row">
              <Badge tone={(v.tone as never) ?? 'default'}>{v.text}</Badge>
              {/* The provenance in full, not as a tooltip: a number you have to
                  hover to attribute is one that gets quoted unattributed. */}
              <span className="dim metric-source">{attribution(v)}</span>
            </div>
          ))}
        </div>
      </dd>
    </>
  )
}

/** How many references the panel shows before asking. A bibliography runs to
 *  dozens; the panel is a column beside the library, not a page. */
const REFERENCE_ROWS = 8

function ItemReferences({ itemKey: selected }: { itemKey: string }) {
  const t = useT()
  // Debounced: arrow-keying down a list must not fetch a bibliography for
  // every row it passes through.
  const itemKey = useDebounced(selected)
  const library = useStore((s) => s.library)
  const openReader = useStore((s) => s.openReader)
  const collection = useStore((s) => s.collection)
  const [list, setList] = useState<CitationList | null>(null)
  const [fetching, setFetching] = useState(false)
  const [filter, setFilter] = useState<'all' | 'held' | 'missing'>('all')
  const [expanded, setExpanded] = useState(false)
  /** DOIs currently being fetched, so a row can say so. */
  const [getting, setGetting] = useState<string[]>([])

  const load = useCallback(() => {
    let live = true
    void api.references
      .list(library, itemKey)
      .then((r) => {
        if (live) setList(r)
      })
      .catch(() => {
        if (live) setList(null)
      })
    return () => {
      live = false
    }
  }, [library, itemKey])

  useEffect(load, [load])

  const fetchRefs = async () => {
    setFetching(true)
    try {
      await api.references.fetch(library, itemKey)
      load()
    } catch (e) {
      toast.fromError(t('detail.referencesFailed'), e)
    } finally {
      setFetching(false)
    }
  }

  const cites = list?.cites ?? []
  const held = cites.filter((c) => c.key)
  const missing = cites.filter((c) => !c.key)

  const shown = filter === 'held' ? held : filter === 'missing' ? missing : cites
  const capped = expanded ? shown : shown.slice(0, REFERENCE_ROWS)

  /** Fetch a cited work the library does not hold, by its DOI. */
  const get = async (c: Citation) => {
    if (!c.doi) return
    setGetting((g) => [...g, c.doi])
    try {
      await api.scrape.quickAdd(library, { text: c.doi })
      load()
    } catch (e) {
      toast.fromError(t('detail.referenceGetFailed'), e)
    } finally {
      setGetting((g) => g.filter((d) => d !== c.doi))
    }
  }

  const file = async (c: Citation) => {
    if (!c.key || !collection) return
    try {
      await api.items.addToCollection(library, collection, [c.key])
      toast.success(t('detail.referenceFiled'))
    } catch (e) {
      toast.fromError(t('detail.referenceFileFailed'), e)
    }
  }

  return (
    <>
      <dt>{t('detail.references')}</dt>
      <dd>
        {cites.length === 0 ? (
          <div className="chip-row">
            <span className="dim">{t('detail.referencesNone')}</span>
            <button className="chip" disabled={fetching} onClick={() => void fetchRefs()}>
              {fetching ? t('detail.referencesFetching') : t('detail.referencesFetch')}
            </button>
          </div>
        ) : (
          <div className="reference-list">
            {/* A bibliography is long -- ninety-three references is ordinary --
                so the panel says what it has and shows a handful, rather than
                becoming a page of grey text you have to scroll past to reach
                anything else. */}
            <div className="reference-head">
              <span className="dim">
                {t('detail.referencesHeld', { held: held.length, total: cites.length })}
              </span>
              <span className="chip-row">
                {(['all', 'held', 'missing'] as const).map((f) => (
                  <button
                    key={f}
                    className="chip"
                    data-active={filter === f || undefined}
                    onClick={() => setFilter(f)}
                  >
                    {t(`detail.references.${f}`)}
                  </button>
                ))}
              </span>
            </div>

            {capped.map((c) => (
              <div key={c.position} className="reference-row" title={c.label}>
                <span className="dim mono">{c.position + 1}</span>
                {c.key ? (
                  <button className="link" onClick={() => openReader(c.key!)}>
                    {c.label || c.fingerprint}
                  </button>
                ) : (
                  <span className="reference-absent">{c.label || c.fingerprint}</span>
                )}
                {c.year && <span className="dim">{c.year}</span>}
                <span className="reference-actions">
                  {/* Held: file it where you are working. Missing: go and get
                      it. Either way the next thing you would do is one click,
                      not a copied DOI and a trip to the search box. */}
                  {c.key && collection && (
                    <button
                      className="icon-btn"
                      title={t('detail.referenceFile')}
                      onClick={() => void file(c)}
                    >
                      <Icon.Folder size={11} />
                    </button>
                  )}
                  {!c.key && c.doi && (
                    <button
                      className="icon-btn"
                      title={t('detail.referenceGet')}
                      disabled={getting.includes(c.doi)}
                      onClick={() => void get(c)}
                    >
                      <Icon.Download size={11} />
                    </button>
                  )}
                </span>
              </div>
            ))}

            {shown.length > REFERENCE_ROWS && (
              <button className="chip" onClick={() => setExpanded(!expanded)}>
                {expanded
                  ? t('detail.referencesFewer')
                  : t('detail.referencesMore', { count: shown.length - REFERENCE_ROWS })}
              </button>
            )}
          </div>
        )}
      </dd>
    </>
  )
}

/** The first page of the paper's PDF, if it has one.
 *
 *  A cover is the fastest way to recognise a paper you have read — faster than
 *  the title, which is why every reference manager grew one. It costs a cache
 *  hit after the first view; see `lib/thumbnails.ts`.
 */
function ItemCover({ itemKey: selected }: { itemKey: string }) {
  const itemKey = useDebounced(selected)
  const library = useStore((s) => s.library)
  const openReader = useStore((s) => s.openReader)
  const [pdf, setPdf] = useState<string | null>(null)

  useEffect(() => {
    let live = true
    void api.items
      .children(library, itemKey)
      .then((kids) => {
        const found = kids.find(
          (k) => k.itemType === 'attachment' && String(k.contentType ?? '').includes('pdf'),
        )
        if (live) setPdf(found?.key ?? null)
      })
      .catch(() => {
        if (live) setPdf(null)
      })
    return () => {
      live = false
    }
  }, [library, itemKey])

  if (!pdf) return null

  return (
    <button className="cover" onClick={() => openReader(pdf)}>
      <Thumbnail library={library} attachmentKey={pdf} width={240} />
    </button>
  )
}

/** The notes written under a paper, including any generated summary.
 *
 *  Summarising has landed a note under the item since it was built, and
 *  nothing showed it: you had to know to go looking at the item's children.
 *  A result nobody is shown is a result that did not happen — and the
 *  assistant has been reading these all along.
 */
function ItemNotes({ itemKey: selected }: { itemKey: string }) {
  const t = useT()
  const itemKey = useDebounced(selected)
  const library = useStore((s) => s.library)
  const openNote = useStore((s) => s.openNote)
  const addNote = useStore((s) => s.addNote)
  const [notes, setNotes] = useState<Item[]>([])

  useEffect(() => {
    let live = true
    void api.items
      .children(library, itemKey)
      .then((kids) => {
        if (live) setNotes(kids.filter((k) => k.itemType === 'note'))
      })
      .catch(() => {
        if (live) setNotes([])
      })
    return () => {
      live = false
    }
  }, [library, itemKey])

  // Shown even with nothing in it. This returned null when a paper had no
  // notes, so the one place you would go to write your first note was the one
  // place that disappeared until you already had one.
  return (
    <>
      <dt>{t('detail.notes')}</dt>
      <dd>
        <div className="note-list">
          {notes.map((note) => {
            const generated = note.tags.some((tag) => tag.tag === 'summary')
            return (
              <button
                key={note.key}
                className="note-row"
                onClick={() => openNote(note.key, plainText(String(note.note ?? '')).slice(0, 40))}
                title={plainText(String(note.note ?? ''))}
              >
                {/* Marked, because a summary the model wrote and a note the
                    user wrote are different things to trust. */}
                {generated && <span className="note-badge">{t('detail.noteGenerated')}</span>}
                <span className="note-text">{plainText(String(note.note ?? ''))}</span>
              </button>
            )
          })}
          <button className="note-row add" onClick={() => void addNote(selected)}>
            <Icon.Plus className="glyph" />
            <span className="note-text">{t('note.add')}</span>
          </button>
        </div>
      </dd>
    </>
  )
}

/** A note's text without its markup, for a one-line preview. */
function plainText(html: string): string {
  return html
    .replace(/<[^>]*>/g, ' ')
    .replace(/&nbsp;/g, ' ')
    .replace(/&amp;/g, '&')
    .replace(/\s+/g, ' ')
    .trim()
}
