import { useEffect, useRef, useState } from 'react'
import { useT } from '../i18n'
import { HIGHLIGHT_COLOURS, type Annotation, type HighlightColour } from '../lib/annotations'
import { failureOf, failureText, type Failure } from '../lib/errors'
import type { ScreenRect } from '../lib/selection-toolbar'
import { Button, Icon, toast } from '../ui'
import { FloatingTools } from './FloatingTools'

export interface AnnotationPatch {
  annotationColor?: HighlightColour
  annotationComment?: string
}

export function AnnotationTools({ annotation, boxes, at, onChange, onNote, onDelete, onDismiss }: {
  annotation: Annotation
  boxes: ScreenRect[]
  at: { x: number; y: number }
  onChange: (patch: AnnotationPatch) => Promise<void>
  onNote: () => Promise<void>
  onDelete: () => Promise<void>
  onDismiss: () => void
}) {
  const t = useT()
  const [editing, setEditing] = useState(false)
  const [comment, setComment] = useState(annotation.comment)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<Failure | null>(null)
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false }
  }, [])
  const run = async (action: () => Promise<void>) => {
    if (busy) return
    setBusy(true)
    setError(null)
    try {
      await action()
    } catch (cause) {
      if (mounted.current) setError(failureOf(cause))
      else toast.fromError(t('reader.annotationFailed'), cause)
    } finally {
      if (mounted.current) setBusy(false)
    }
  }
  return (
    <FloatingTools at={at} selection={boxes} label={t('reader.annotationTools')}
      onDismiss={onDismiss} className={`annotation-tools${editing ? ' editing' : ''}`}>
      <div className="annotation-tool-row">
        <div className="swatches">
          {HIGHLIGHT_COLOURS.map((colour) => (
            <button key={colour} className="swatch" data-colour={colour}
              data-active={colour === annotation.colour} disabled={busy}
              aria-label={t('reader.annotationColour', { colour: t(`collection.colour.${colour}`) })}
              title={t('reader.annotationColour', { colour: t(`collection.colour.${colour}`) })}
              onClick={() => void run(() => onChange({ annotationColor: colour }))} />
          ))}
        </div>
        <button className="popup-action" disabled={busy} title={t('reader.annotationComment')}
          aria-label={t('reader.annotationComment')} aria-expanded={editing}
          onClick={() => setEditing((value) => !value)}><Icon.Chat size={13} /></button>
        <button className="popup-action" disabled={busy} title={t('reader.noteFromMark')}
          aria-label={t('reader.noteFromMark')} onClick={() => void run(onNote)}><Icon.Note size={13} /></button>
        <button className="popup-action" disabled={busy} title={t('menu.delete')}
          aria-label={t('menu.delete')} onClick={() => void run(onDelete)}><Icon.Trash size={13} /></button>
        <button className="popup-action" title={t('dialog.cancel')} aria-label={t('dialog.cancel')}
          onClick={onDismiss}><Icon.Close size={12} /></button>
      </div>
      {editing && (
        <form className="annotation-comment" onSubmit={(event) => {
          event.preventDefault()
          void run(async () => {
            await onChange({ annotationComment: comment })
            setEditing(false)
          })
        }}>
          <label>
            {t('reader.annotationComment')}
            <textarea value={comment} rows={4} disabled={busy} autoFocus
              onChange={(event) => setComment(event.target.value)} />
          </label>
          <Button type="submit" disabled={busy}>{t('reader.saveAnnotation')}</Button>
        </form>
      )}
      {error && <div className="err annotation-error" role="alert" title={error.detail}>
        {failureText(t, error)}
      </div>}
    </FloatingTools>
  )
}
