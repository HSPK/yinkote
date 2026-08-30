/**
 *  Writing a note on a paper.
 *
 *  A tab rather than a dialog, because a note is work and not a preference: it
 *  is written over minutes, alongside the paper, and it must survive opening
 *  something else and coming back. That is what tabs are for here.
 *
 *  Edit and preview rather than a live-rendered editor. Markdown that rewrites
 *  itself under the cursor is a much larger thing to build and a worse thing to
 *  type in; a pane you can flip is honest about what is stored, which is the
 *  markdown itself.
 */
import { useCallback, useEffect, useRef, useState } from 'react'

import { api } from '../api/client'
import type { Item } from '../api/types'
import { useT } from '../i18n'
import { failureOf, failureText, type Failure } from '../lib/errors'
import { Markdown } from '../lib/markdown'
import { useStore } from '../state/store'
import { Button, Empty } from '../ui/controls'

/** How long typing pauses before a draft is written. */
const AUTOSAVE_MS = 1200

export function NoteView({ target }: { target?: string }) {
  const t = useT()
  const library = useStore((s) => s.library)
  const [note, setNote] = useState<Item | null>(null)
  const [text, setText] = useState('')
  const [preview, setPreview] = useState(false)
  const [saving, setSaving] = useState<'idle' | 'saving' | 'saved'>('idle')
  const [error, setError] = useState<Failure | null>(null)

  // What is on the server, and *which note* it belongs to. The key is half of
  // it: a pending autosave from one note must never land on another, which is
  // possible whenever one instance is reused for a new target.
  const stored = useRef({ key: '', body: '' })

  useEffect(() => {
    if (!target) return
    let live = true
    void api.items
      .get(library, target)
      .then((got) => {
        if (!live) return
        setNote(got)
        const body = String(got.note ?? '')
        setText(body)
        stored.current = { key: target, body }
        setError(null)
      })
      .catch((e: unknown) => live && setError(failureOf(e)))
    return () => {
      live = false
    }
  }, [library, target])

  const save = useCallback(
    async (body: string) => {
      // Belongs to another note, or changes nothing.
      if (!target || stored.current.key !== target || body === stored.current.body) return
      setSaving('saving')
      try {
        // `fields` is nested on a patch, not flattened (3.217).
        const updated = await api.items.update(library, target, { fields: { note: body } })
        stored.current = { key: target, body }
        setNote(updated)
        setSaving('saved')
      } catch (e: unknown) {
        setError(failureOf(e))
        setSaving('idle')
      }
    },
    [library, target],
  )

  // Saved on a pause rather than on a button, because a note nobody
  // remembered to save is a note that was not written. The button stays for
  // the moment before the pause elapses.
  useEffect(() => {
    if (!target || stored.current.key !== target || text === stored.current.body) return
    const timer = setTimeout(() => void save(text), AUTOSAVE_MS)
    return () => clearTimeout(timer)
  }, [text, target, save])

  // And on the way out, because the pause has not elapsed when a tab closes.
  //
  // Through a ref, and with no dependencies. Written the obvious way — an
  // effect depending on `text` — the cleanup runs on *every* keystroke
  // carrying the text from before it, and the first of those fires when the
  // fetch resolves: it saved the empty string the editor was born with over
  // the note that had just loaded. An autosave that erases is worse than none.
  const latest = useRef({ text, save })
  latest.current = { text, save }
  useEffect(
    () => () => {
      const { text, save } = latest.current
      if (text !== stored.current.body) void save(text)
    },
    [],
  )

  if (!target) return <Empty>{t('note.none')}</Empty>
  if (error && !note) return <Empty title={error.detail}>{failureText(t, error)}</Empty>

  const generated = note?.tags?.some((tag) => tag.tag === 'summary' || tag.tag === 'close-reading')

  return (
    <div className="note-view">
      <div className="note-bar">
        <div className="note-modes">
          <Button tone={preview ? 'ghost' : 'primary'} onClick={() => setPreview(false)}>
            {t('note.write')}
          </Button>
          <Button tone={preview ? 'primary' : 'ghost'} onClick={() => setPreview(true)}>
            {t('note.preview')}
          </Button>
        </div>
        <span className="note-state dim">
          {saving === 'saving' && t('note.saving')}
          {saving === 'saved' && t('note.saved')}
          {/* Said, because a note the model wrote and a note the reader wrote
              are different things to trust — and editing one makes it yours. */}
          {generated && ` · ${t('detail.noteGenerated')}`}
        </span>
        {error && (
          <span className="err" title={error.detail}>
            {failureText(t, error)}
          </span>
        )}
      </div>

      {preview ? (
        <div className="note-preview">
          {text.trim() ? <Markdown source={text} /> : <Empty>{t('note.empty')}</Empty>}
        </div>
      ) : (
        <textarea
          className="note-editor"
          value={text}
          spellCheck={false}
          placeholder={t('note.placeholder')}
          onChange={(e) => setText(e.target.value)}
        />
      )}
    </div>
  )
}
