/** The same note editor serves a workspace tab and a reader's sidebar. */
import { useEffect, useRef, useState } from 'react'

import type { Item } from '../api/types'
import { useT } from '../i18n'
import { failureText } from '../lib/errors'
import { Markdown } from '../lib/markdown'
import { useStore } from '../state/store'
import { Button, Empty, Input, Toggle } from '../ui/controls'
import { Icon } from '../ui/Icon'
import { useNoteDraft } from './useNoteDraft'
import './note-editor.css'

export interface NoteViewProps {
  target?: string
  library?: number
  embedded?: boolean
  initialPreview?: boolean
  onSaved?: (note: Item) => void
  onBack?: () => void
}

export function NoteView({
  target,
  library: ownerLibrary,
  embedded = false,
  initialPreview = false,
  onSaved,
  onBack,
}: NoteViewProps) {
  const t = useT()
  const selectedLibrary = useStore((s) => s.library)
  const library = ownerLibrary ?? selectedLibrary
  const draft = useNoteDraft(library, target, onSaved)
  const [preview, setPreview] = useState(initialPreview)
  const [leaving, setLeaving] = useState(false)
  const backGeneration = useRef(0)

  useEffect(() => {
    setPreview(initialPreview)
    setLeaving(false)
    return () => { backGeneration.current += 1 }
  }, [library, target, initialPreview])

  if (!target) return <Empty>{t('note.none')}</Empty>

  const conflict = draft.error?.code === 'version_conflict'

  return (
    <div
      className={`note-view note-workspace${embedded ? ' note-embedded' : ''}`}
      aria-busy={draft.loading || leaving}
    >
      <div className="note-title-row">
        {onBack && (
          <Button
            tone="ghost"
            className="note-back"
            title={t('note.back')}
            aria-label={t('note.back')}
            disabled={leaving}
            onClick={async () => {
              const generation = backGeneration.current
              setLeaving(true)
              const saved = await draft.flush()
              if (backGeneration.current !== generation) return
              if (saved) onBack()
              setLeaving(false)
            }}
          >
            <Icon.ChevronLeft size={14} />
          </Button>
        )}
        <Input
          className="note-title-input"
          aria-label={t('note.titleLabel')}
          placeholder={t('note.titlePlaceholder')}
          value={draft.title}
          disabled={!draft.note || leaving}
          onChange={(e) => draft.edit('title', e.target.value)}
        />
      </div>
      <div className="note-bar">
        <label className="note-preview-toggle">
          <span>{t('note.preview')}</span>
          <Toggle checked={preview} disabled={!draft.note} onChange={setPreview} />
        </label>
        <span className="note-state dim" role="status">
          {draft.loading
            ? t('note.loading')
            : draft.saving
              ? t('note.saving')
              : draft.saved
                ? t('note.saved')
                : draft.dirty
                  ? t('note.unsaved')
                  : ''}
        </span>
      </div>
      {draft.error && (
        <div className="note-error" role="alert">
          <span className="err" title={draft.error.detail}>
            {failureText(t, draft.error)}
            {conflict && ` ${t('note.conflictHint')}`}
          </span>
          <Button disabled={draft.saving || draft.loading || leaving} onClick={draft.retry}>
            {t(conflict ? 'note.keepChanges' : 'error.retry')}
          </Button>
        </div>
      )}
      {preview ? (
        <div className="note-preview">
          {draft.loading ? (
            <Empty>{t('note.loading')}</Empty>
          ) : draft.body.trim() ? (
            <Markdown source={draft.body} />
          ) : (
            <Empty>{t('note.empty')}</Empty>
          )}
        </div>
      ) : (
        <textarea
          className="note-editor"
          aria-label={t('note.bodyLabel')}
          value={draft.body}
          disabled={!draft.note || leaving}
          spellCheck={false}
          placeholder={t(draft.loading ? 'note.loading' : 'note.placeholder')}
          onChange={(e) => draft.edit('body', e.target.value)}
        />
      )}
    </div>
  )
}
